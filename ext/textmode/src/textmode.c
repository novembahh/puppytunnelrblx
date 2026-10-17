#include <windows.h>
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
static volatile LONG g_frames = 0;
static volatile LONG g_forwarded = 0;

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

static void *tm_iat_hook(const char *dll, const char *func, void *repl) {
    BYTE *base = (BYTE *)GetModuleHandleA(NULL);
    IMAGE_DOS_HEADER *dos;
    IMAGE_NT_HEADERS *nt;
    DWORD rva;
    IMAGE_IMPORT_DESCRIPTOR *imp;
    HMODULE target;
    void *want;
    if (!base || !repl) return NULL;
    dos = (IMAGE_DOS_HEADER *)base;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return NULL;
    nt = (IMAGE_NT_HEADERS *)(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return NULL;
    rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    if (!rva) return NULL;
    target = GetModuleHandleA(dll);
    if (!target) return NULL;
    want = (void *)GetProcAddress(target, func);
    if (!want) return NULL;
    imp = (IMAGE_IMPORT_DESCRIPTOR *)(base + rva);
    for (; imp->Name; imp++) {
        const char *name = (const char *)(base + imp->Name);
        IMAGE_THUNK_DATA *thunk;
        if (_stricmp(name, dll) != 0) continue;
        thunk = (IMAGE_THUNK_DATA *)(base + imp->FirstThunk);
        for (; thunk->u1.Function; thunk++) {
            void **entry = (void **)&thunk->u1.Function;
            if (*entry == want) {
                DWORD old;
                if (!VirtualProtect(entry, sizeof(void *), PAGE_READWRITE, &old))
                    return NULL;
                *entry = repl;
                VirtualProtect(entry, sizeof(void *), old, &old);
                return want;
            }
        }
    }
    return NULL;
}

#define MAX_VP 64
struct VPatch { void *vt; int idx; void *orig; };
static struct VPatch g_vp[MAX_VP];
static int g_vpn = 0;

static void *tm_vtable_hook(void *obj, int index, void *repl) {
    void **vt;
    void *orig;
    DWORD old;
    int i;
    if (!obj || !repl) return NULL;
    vt = *(void ***)obj;
    if (!vt) return NULL;
    for (i = 0; i < g_vpn; i++)
        if (g_vp[i].vt == (void *)vt && g_vp[i].idx == index)
            return g_vp[i].orig;
    orig = vt[index];
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

static const void *tm_sigscan(const void *base, size_t size, const char *pat, const char *mask) {
    size_t i, plen = strlen(mask);
    const unsigned char *b = (const unsigned char *)base;
    size_t n;
    if (plen == 0 || size < plen) return NULL;
    n = size - plen;
    for (i = 0; i <= n; i++) {
        size_t j;
        for (j = 0; j < plen; j++)
            if (mask[j] == 'x' && pat[j] != (char)b[i + j])
                break;
        if (j == plen) return b + i;
    }
    return NULL;
}

struct SigTarget { const char *name; const char *pat; const char *mask; };
static const struct SigTarget sig_targets[] = { { NULL, NULL, NULL } };

static void tm_clamp(UINT *w, UINT *h) {
    UINT mw = *w, mh = *h, nw, nh;
    double s;
    if (mw == 0 || mh == 0) return;
    if (mw <= g_maxdim && mh <= g_maxdim) return;
    s = (double)g_maxdim / (double)(mw > mh ? mw : mh);
    nw = (UINT)(mw * s); if (!nw) nw = 1;
    nh = (UINT)(mh * s); if (!nh) nh = 1;
    tm_log("[textmode] clamp backbuffer %ux%u -> %ux%u", mw, mh, nw, nh);
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
    (void)self;
    if (g_skip > 0 && (f % (g_skip + 1)) != 0)
        return S_OK;
    fw = InterlockedIncrement(&g_forwarded);
    if (fw == 1 || fw % 600 == 0)
        tm_log("[textmode] frames=%ld presented=%ld", (long)f, (long)fw);
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
    tm_log("[textmode] swapchain hooked: %p", sc);
}

typedef HRESULT (WINAPI *DCDASCFn)(void *, int, void *, UINT, const void *, UINT, UINT,
                                   const void *, void **, void **, void *, void **);
static DCDASCFn g_dcdasc_orig = NULL;

static HRESULT WINAPI D3D11CreateDeviceAndSwapChain_hook(void *adapter, int dtype, void *sw,
        UINT flags, const void *fl, UINT fln, UINT sdk, const void *sdesc,
        void **sc, void **dev, void *outfl, void **ctx) {
    HRESULT hr;
    if (sdesc) {
        DXGI_SWAP_CHAIN_DESC d;
        UINT w, h;
        memcpy(&d, sdesc, sizeof(d));
        w = d.BufferDesc.Width;
        h = d.BufferDesc.Height;
        tm_clamp(&w, &h);
        d.BufferDesc.Width = w;
        d.BufferDesc.Height = h;
        tm_log("[textmode] D3D11CreateDeviceAndSwapChain backbuffer %ux%u", w, h);
        hr = g_dcdasc_orig(adapter, dtype, sw, flags, fl, fln, sdk, &d, sc, dev, outfl, ctx);
    } else {
        hr = g_dcdasc_orig(adapter, dtype, sw, flags, fl, fln, sdk, sdesc, sc, dev, outfl, ctx);
    }
    if (hr >= 0 && sc) tm_hook_swapchain(*sc);
    return hr;
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
        tm_log("[textmode] IDXGIFactory::CreateSwapChain backbuffer %ux%u", w, h);
        hr = g_csc_orig(self, dev, &d, sc);
    } else {
        hr = g_csc_orig(self, dev, desc, sc);
    }
    if (hr >= 0 && sc) tm_hook_swapchain(*sc);
    return hr;
}

typedef HRESULT (WINAPI *CDFn)(const void *, void **);
typedef HRESULT (WINAPI *CDF2n)(UINT, const void *, void **);
static CDFn g_cdf_orig = NULL, g_cdf1_orig = NULL;
static CDF2n g_cdf2_orig = NULL;

static void tm_hook_factory(void *f) {
    if (!f) return;
    g_csc_orig = (CreateSwapChainFn)tm_vtable_hook(f, 10, (void *)CreateSwapChain_hook);
    tm_log("[textmode] factory hooked: %p", f);
}

static HRESULT WINAPI CreateDXGIFactory_hook(const void *riid, void **f) {
    HRESULT hr = g_cdf_orig(riid, f);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

static HRESULT WINAPI CreateDXGIFactory1_hook(const void *riid, void **f) {
    HRESULT hr = g_cdf1_orig(riid, f);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

static HRESULT WINAPI CreateDXGIFactory2_hook(UINT flags, const void *riid, void **f) {
    HRESULT hr = g_cdf2_orig(flags, riid, f);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

static DWORD WINAPI init_thread(LPVOID unused) {
    char path[MAX_PATH * 2];
    DWORD n;
    int i, have_all;
    (void)unused;
    InitializeCriticalSection(&g_log_cs);
    g_log_ready = 1;
    g_maxdim = (UINT)tm_env_int("TEXTMODE_MAXDIM", 64);
    if (g_maxdim < 8) g_maxdim = 8;
    if (g_maxdim > 1024) g_maxdim = 1024;
    g_skip = tm_env_int("TEXTMODE_SKIP", 0);
    if (g_skip < 0) g_skip = 0;
    if (g_skip > 6000) g_skip = 6000;
    n = GetEnvironmentVariableA("TEXTMODE_LOG", path, sizeof(path));
    if (n == 0 || n >= sizeof(path)) {
        char tmp[MAX_PATH];
        GetTempPathA(sizeof(tmp), tmp);
        snprintf(path, sizeof(path), "%stextmode-%lu.log", tmp, (unsigned long)GetCurrentProcessId());
    }
    g_log = fopen(path, "a");
    tm_log("[textmode] attach pid=%lu maxdim=%u skip=%d log=%s",
           (unsigned long)GetCurrentProcessId(), g_maxdim, g_skip, path);
    (void)tm_sigscan;
    (void)sig_targets;
    for (i = 0; i < 240; i++) {
        have_all = 1;
        if (!g_dcdasc_orig) {
            void *o = tm_iat_hook("d3d11.dll", "D3D11CreateDeviceAndSwapChain",
                                  (void *)D3D11CreateDeviceAndSwapChain_hook);
            if (o) { g_dcdasc_orig = (DCDASCFn)o; tm_log("[textmode] hooked D3D11CreateDeviceAndSwapChain"); }
            else have_all = 0;
        }
        if (!g_cdf_orig) {
            void *o = tm_iat_hook("dxgi.dll", "CreateDXGIFactory", (void *)CreateDXGIFactory_hook);
            if (o) { g_cdf_orig = (CDFn)o; tm_log("[textmode] hooked CreateDXGIFactory"); }
            else have_all = 0;
        }
        if (!g_cdf1_orig) {
            void *o = tm_iat_hook("dxgi.dll", "CreateDXGIFactory1", (void *)CreateDXGIFactory1_hook);
            if (o) { g_cdf1_orig = (CDFn)o; tm_log("[textmode] hooked CreateDXGIFactory1"); }
            else have_all = 0;
        }
        if (!g_cdf2_orig) {
            void *o = tm_iat_hook("dxgi.dll", "CreateDXGIFactory2", (void *)CreateDXGIFactory2_hook);
            if (o) { g_cdf2_orig = (CDF2n)o; tm_log("[textmode] hooked CreateDXGIFactory2"); }
            else have_all = 0;
        }
        if (have_all) break;
        Sleep(500);
    }
    tm_log("[textmode] init done (d3d11=%d factory=%d f1=%d f2=%d)",
           !!g_dcdasc_orig, !!g_cdf_orig, !!g_cdf1_orig, !!g_cdf2_orig);
    return 0;
}

BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    HANDLE t;
    char en[16];
    DWORD n;
    (void)x;
    if (r != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(m);
    n = GetEnvironmentVariableA("TEXTMODE", en, sizeof(en));
    if (n > 0 && n < sizeof(en) && (en[0] == '0' || en[0] == 'n' || en[0] == 'N'))
        return TRUE;
    t = CreateThread(NULL, 0, init_thread, NULL, 0, NULL);
    if (t) CloseHandle(t);
    return TRUE;
}
