#include <windows.h>
#include <intrin.h>
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include <dxgi.h>
#include <d3d11.h>

static FILE *g_log = NULL;
static CRITICAL_SECTION g_log_cs;
static int g_log_ready = 0;

static UINT g_maxdim = 64;
static int g_skip = 0;
static int g_passthrough = 0;
static volatile LONG g_frames = 0;
static volatile LONG g_forwarded = 0;
static const GUID TM_IID_DXGIDEV = {0x54ec77fa, 0x1377, 0x44e6, {0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c}};
static const GUID TM_IID_DXGIFAC = {0x7b7166ec, 0x21c7, 0x44ae, {0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69}};

static void tm_log(const char *fmt, ...) {
    va_list ap;
    if (!g_log) return;
    if (g_log_ready) EnterCriticalSection(&g_log_cs);
    va_start(ap, fmt);
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fputc('\n', g_log);
    fflush(g_log);
    if (g_log_ready) LeaveCriticalSection(&g_log_cs);
}

static int tm_env_int(const char *name, int def) {
    char buf[32];
    DWORD n = GetEnvironmentVariableA(name, buf, sizeof(buf));
    if (n == 0 || n >= sizeof(buf)) return def;
    return atoi(buf);
}

static void tm_log_open(const char *tag) {
    char path[MAX_PATH * 2];
    DWORD n;
    InitializeCriticalSection(&g_log_cs);
    g_log_ready = 1;
    g_maxdim = (UINT)tm_env_int("TEXTMODE_MAXDIM", 64);
    if (g_maxdim < 8) g_maxdim = 8;
    if (g_maxdim > 1024) g_maxdim = 1024;
    g_skip = tm_env_int("TEXTMODE_SKIP", 0);
    g_passthrough = tm_env_int("TEXTMODE_PASSTHROUGH", 0);
    if (g_skip < 0) g_skip = 0;
    if (g_skip > 6000) g_skip = 6000;
    n = GetEnvironmentVariableA("TEXTMODE_LOG", path, sizeof(path));
    if (n == 0 || n >= sizeof(path)) {
        char tmp[MAX_PATH];
        GetTempPathA(sizeof(tmp), tmp);
        snprintf(path, sizeof(path), "%s%s-%lu.log", tmp, tag, (unsigned long)GetCurrentProcessId());
    }
    g_log = fopen(path, "a");
    tm_log("[proxy-%s] attach pid=%lu maxdim=%u skip=%d", tag,
           (unsigned long)GetCurrentProcessId(), g_maxdim, g_skip);
}

static HMODULE tm_load_real(const char *name) {
    char sys[MAX_PATH];
    char full[MAX_PATH * 2];
    HMODULE h;
    if (!GetSystemDirectoryA(sys, sizeof(sys))) return NULL;
    snprintf(full, sizeof(full), "%s\\%s", sys, name);
    h = LoadLibraryA(full);
    tm_log("[proxy] load real %s -> %p", full, (void *)h);
    return h;
}


#define MAX_VP 64
typedef struct { unsigned short Length, MaximumLength; wchar_t *Buffer; } TM_USTR;
typedef struct _TM_LDR_ENTRY {
    struct _TM_LDR_ENTRY *Flink;
    struct _TM_LDR_ENTRY *Blink;
    struct _TM_LDR_ENTRY *MemFlink;
    struct _TM_LDR_ENTRY *MemBlink;
    struct _TM_LDR_ENTRY *InitFlink;
    struct _TM_LDR_ENTRY *InitBlink;
    void *DllBase;
    void *EntryPoint;
    unsigned long SizeOfImage;
    TM_USTR FullDllName;
    TM_USTR BaseDllName;
} TM_LDR_ENTRY;
static HMODULE g_selfm = NULL;
static int tm_ptr_valid(const void *p, int exec);
static void tm_hide_self(void) {
    unsigned long long peb = __readgsqword(0x60);
    unsigned char *ldr = *(unsigned char **)(peb + 24);
    TM_LDR_ENTRY *head, *cur;
    HMODULE self = g_selfm;
    tm_log("[proxy] hide probe peb=%p ldr=%p self=%p", (void *)peb, ldr, self);
    if (!ldr) return;
    if (!self) return;
    head = (TM_LDR_ENTRY *)(ldr + 16);
    {
        int n = 0, found = 0;
        cur = head->Flink;
        while (cur && cur != head && n < 8192) {
            TM_LDR_ENTRY *next;
            if (!tm_ptr_valid(cur, 0)) break;
            if (cur->DllBase == self) found = 1;
            n++;
            next = cur->Flink;
            cur = next;
        }
        tm_log("[proxy] hide walk entries=%d found_self=%d", n, found);
    }
}
static int tm_ptr_valid(const void *p, int exec) {
    MEMORY_BASIC_INFORMATION mi;
    DWORD pr;
    if (!p) return 0;
    if (VirtualQuery(p, &mi, sizeof(mi)) != sizeof(mi)) return 0;
    if (mi.State != MEM_COMMIT) return 0;
    pr = mi.Protect & 0xff;
    if (pr != PAGE_READONLY && pr != PAGE_READWRITE && pr != PAGE_WRITECOPY
        && pr != PAGE_EXECUTE_READ && pr != PAGE_EXECUTE_READWRITE && pr != PAGE_EXECUTE_WRITECOPY)
        return 0;
    if (exec && pr != PAGE_EXECUTE_READ && pr != PAGE_EXECUTE_READWRITE && pr != PAGE_EXECUTE_WRITECOPY)
        return 0;
    return 1;
}
struct VPatch { void *vt; int idx; void *orig; };
static struct VPatch g_vp[MAX_VP];
static int g_vpn = 0;

static void *tm_vtable_hook(void *obj, int index, void *repl) {
    void **vt;
    void *orig;
    DWORD old;
    int i;
    if (!obj || !repl) return NULL;
    if (!tm_ptr_valid(obj, 0)) { tm_log("[proxy] skip hook: obj %p unreadable", obj); return NULL; }
    vt = *(void ***)obj;
    if (!tm_ptr_valid(vt, 0)) { tm_log("[proxy] skip hook: vt %p unreadable", vt); return NULL; }
    for (i = 0; i < g_vpn; i++)
        if (g_vp[i].vt == (void *)vt && g_vp[i].idx == index)
            return g_vp[i].orig;
    orig = vt[index];
    if (!tm_ptr_valid(orig, 1)) { tm_log("[proxy] skip hook: slot %d target %p not code", index, orig); return NULL; }
    if (orig == repl) return orig;
    if (!VirtualProtect(&vt[index], sizeof(void *), PAGE_EXECUTE_READWRITE, &old))
        return NULL;
    vt[index] = repl;
    VirtualProtect(&vt[index], sizeof(void *), old, &old);
    if (g_vpn < MAX_VP) {
        g_vp[g_vpn].vt = (void *)vt;
        g_vp[g_vpn].idx = index;
        g_vp[g_vpn].orig = orig;
        g_vpn++;
    }
    return orig;
}

static void tm_clamp(UINT *w, UINT *h) {
    UINT mw = *w, mh = *h, nw, nh;
    double s;
    if (mw == 0 || mh == 0) return;
    if (mw <= g_maxdim && mh <= g_maxdim) return;
    s = (double)g_maxdim / (double)(mw > mh ? mw : mh);
    nw = (UINT)(mw * s); if (!nw) nw = 1;
    nh = (UINT)(mh * s); if (!nh) nh = 1;
    tm_log("[proxy] clamp backbuffer %ux%u -> %ux%u", mw, mh, nw, nh);
    *w = nw;
    *h = nh;
}

typedef HRESULT (STDMETHODCALLTYPE *PresentFn)(void *, UINT, UINT);
typedef HRESULT (STDMETHODCALLTYPE *ResizeBuffersFn)(void *, UINT, UINT, UINT, DXGI_FORMAT, UINT);
static PresentFn g_present_orig = NULL;
static ResizeBuffersFn g_rb_orig = NULL;

static HRESULT STDMETHODCALLTYPE Present_hook(void *self, UINT si, UINT fl) {
    LONG f = InterlockedIncrement(&g_frames);
    LONG fw;
    if (g_skip > 0 && (f % (g_skip + 1)) != 0)
        return S_OK;
    fw = InterlockedIncrement(&g_forwarded);
    if (fw == 1 || fw % 600 == 0)
        tm_log("[proxy] frames=%ld presented=%ld", (long)f, (long)fw);
    return g_present_orig(self, si, fl);
}

static HRESULT STDMETHODCALLTYPE ResizeBuffers_hook(void *self, UINT bc, UINT w, UINT h,
                                                    DXGI_FORMAT fmt, UINT flags) {
    tm_clamp(&w, &h);
    return g_rb_orig(self, bc, w, h, fmt, flags);
}

static void tm_hook_swapchain(void *sc) {
    if (!sc) return;
    g_present_orig = (PresentFn)tm_vtable_hook(sc, 8, (void *)Present_hook);
    g_rb_orig = (ResizeBuffersFn)tm_vtable_hook(sc, 13, (void *)ResizeBuffers_hook);
    tm_log("[proxy] swapchain hooked: %p", sc);
}

typedef HRESULT (STDMETHODCALLTYPE *CreateSwapChainFn)(void *, void *, void *, void **);
static CreateSwapChainFn g_csc_orig = NULL;

static HRESULT STDMETHODCALLTYPE CreateSwapChain_hook(void *self, void *dev, void *desc, void **sc) {
    HRESULT hr;
    if (desc) {
        DXGI_SWAP_CHAIN_DESC d;
        UINT w, h;
        memcpy(&d, desc, sizeof(d));
        w = d.BufferDesc.Width;
        h = d.BufferDesc.Height;
        tm_clamp(&w, &h);
        d.BufferDesc.Width = w;
        d.BufferDesc.Height = h;
        tm_log("[proxy] CreateSwapChain backbuffer %ux%u", w, h);
        hr = g_csc_orig(self, dev, &d, sc);
    } else {
        hr = g_csc_orig(self, dev, desc, sc);
    }
    if (hr >= 0 && sc) tm_hook_swapchain(*sc);
    return hr;
}

static void tm_hook_factory(void *f) {
    if (!f) return;
    if (g_passthrough) { tm_log("[proxy] factory passthrough: %p", f); return; }
    g_csc_orig = (CreateSwapChainFn)tm_vtable_hook(f, 10, (void *)CreateSwapChain_hook);
    tm_log("[proxy] factory hooked: %p", f);
}

#ifdef PROXY_D3D11

typedef HRESULT (WINAPI *D3D11CreateDeviceFn)(IDXGIAdapter *, D3D_DRIVER_TYPE, HMODULE, UINT,
                                              const D3D_FEATURE_LEVEL *, UINT, UINT,
                                              ID3D11Device **, D3D_FEATURE_LEVEL *,
                                              ID3D11DeviceContext **);
static D3D11CreateDeviceFn g_real_create = NULL;

HRESULT WINAPI D3D11CreateDevice(IDXGIAdapter *adapter, D3D_DRIVER_TYPE dtype, HMODULE sw,
        UINT flags, const D3D_FEATURE_LEVEL *fl, UINT fln, UINT sdk,
        ID3D11Device **dev, D3D_FEATURE_LEVEL *outfl, ID3D11DeviceContext **ctx) {
    HRESULT hr;
    tm_log("[proxy] D3D11CreateDevice driver=%d flags=0x%x levels=%u", (int)dtype, flags, fln);
    if (!g_real_create) return E_FAIL;
    hr = g_real_create(adapter, dtype, sw, flags, fl, fln, sdk, dev, outfl, ctx);
    tm_log("[proxy] D3D11CreateDevice -> 0x%08lx device=%p", (unsigned long)hr, dev ? *dev : NULL);
    if (hr >= 0 && dev && *dev) {
        HRESULT (STDMETHODCALLTYPE *qi)(void *, REFIID, void **);
        ULONG (STDMETHODCALLTYPE *rel)(void *);
        HRESULT (STDMETHODCALLTYPE *gp)(void *, REFIID, void **);
        void *dd = NULL;
        qi = (HRESULT (STDMETHODCALLTYPE *)(void *, REFIID, void **))(*(void ***) *dev)[0];
        hr = qi(*dev, &TM_IID_DXGIDEV, &dd);
        tm_log("[proxy] QI IDXGIDevice -> 0x%08lx dev=%p", (unsigned long)hr, dd);
        if (hr >= 0 && dd) {
            void *ad = NULL;
            HRESULT (STDMETHODCALLTYPE *ga)(void *, void **);
            ga = (HRESULT (STDMETHODCALLTYPE *)(void *, void **))(*(void ***)dd)[7];
            hr = ga(dd, &ad);
            tm_log("[proxy] GetAdapter -> 0x%08lx ad=%p", (unsigned long)hr, ad);
            if (hr >= 0 && ad) {
                void *fac = NULL;
                gp = (HRESULT (STDMETHODCALLTYPE *)(void *, REFIID, void **))(*(void ***)ad)[6];
                hr = gp(ad, &TM_IID_DXGIFAC, &fac);
                tm_log("[proxy] Adapter GetParent factory -> 0x%08lx fac=%p", (unsigned long)hr, fac);
                if (hr >= 0 && fac) {
                    tm_hook_factory(fac);
                    rel = (ULONG (STDMETHODCALLTYPE *)(void *))(*(void ***)fac)[2];
                    rel(fac);
                }
                rel = (ULONG (STDMETHODCALLTYPE *)(void *))(*(void ***)ad)[2];
                rel(ad);
            }
            rel = (ULONG (STDMETHODCALLTYPE *)(void *))(*(void ***)dd)[2];
            rel(dd);
        }
    }
    return hr;
}

BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    HMODULE real;
    (void)x;
    if (r != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(m);
    g_selfm = m;
    tm_log_open("d3d11");
    tm_hide_self();
    real = tm_load_real("d3d11.dll");
    if (real) g_real_create = (D3D11CreateDeviceFn)GetProcAddress(real, "D3D11CreateDevice");
    tm_log("[proxy] real D3D11CreateDevice=%p", (void *)g_real_create);
    (void)CreateSwapChain_hook;
    (void)Present_hook;
    (void)ResizeBuffers_hook;
    (void)tm_hook_swapchain;
    return TRUE;
}

#endif

#ifdef PROXY_DXGI

typedef HRESULT (WINAPI *CDFn)(REFIID, void **);
typedef HRESULT (WINAPI *CDFn2)(UINT, REFIID, void **);
static CDFn g_real_factory = NULL;
static CDFn g_real_factory1 = NULL;
static CDFn2 g_real_factory2 = NULL;

HRESULT WINAPI CreateDXGIFactory(REFIID riid, void **f) {
    HRESULT hr;
    if (!g_real_factory) return E_FAIL;
    hr = g_real_factory(riid, f);
    tm_log("[proxy] CreateDXGIFactory -> 0x%08lx factory=%p", (unsigned long)hr, f ? *f : NULL);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

HRESULT WINAPI CreateDXGIFactory1(REFIID riid, void **f) {
    HRESULT hr;
    if (!g_real_factory1) return E_FAIL;
    hr = g_real_factory1(riid, f);
    tm_log("[proxy] CreateDXGIFactory1 -> 0x%08lx factory=%p", (unsigned long)hr, f ? *f : NULL);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

HRESULT WINAPI CreateDXGIFactory2(UINT flags, REFIID riid, void **f) {
    if (!g_real_factory2) return E_FAIL;
    return g_real_factory2(flags, riid, f);
}

BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    HMODULE real;
    (void)x;
    if (r != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(m);
    g_selfm = m;
    tm_log_open("dxgi");
    tm_hide_self();
    real = tm_load_real("dxgi.dll");
    if (real) g_real_factory = (CDFn)GetProcAddress(real, "CreateDXGIFactory");
    if (real) g_real_factory1 = (CDFn)GetProcAddress(real, "CreateDXGIFactory1");
    if (real) g_real_factory2 = (CDFn2)GetProcAddress(real, "CreateDXGIFactory2");
    tm_log("[proxy] real CreateDXGIFactory=%p", (void *)g_real_factory);
    return TRUE;
}

#endif
