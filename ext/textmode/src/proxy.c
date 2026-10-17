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

static void tm_log_open(const char *tag) {
    char path[MAX_PATH * 2];
    DWORD n;
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
    return hr;
}

BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    HMODULE real;
    (void)x;
    if (r != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(m);
    tm_log_open("d3d11");
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
static CDFn g_real_factory = NULL;

static void tm_hook_factory(void *f) {
    if (!f) return;
    g_csc_orig = (CreateSwapChainFn)tm_vtable_hook(f, 10, (void *)CreateSwapChain_hook);
    tm_log("[proxy] factory hooked: %p", f);
}

HRESULT WINAPI CreateDXGIFactory(REFIID riid, void **f) {
    HRESULT hr;
    if (!g_real_factory) return E_FAIL;
    hr = g_real_factory(riid, f);
    tm_log("[proxy] CreateDXGIFactory -> 0x%08lx factory=%p", (unsigned long)hr, f ? *f : NULL);
    if (hr >= 0 && f) tm_hook_factory(*f);
    return hr;
}

BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    HMODULE real;
    (void)x;
    if (r != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(m);
    tm_log_open("dxgi");
    real = tm_load_real("dxgi.dll");
    if (real) g_real_factory = (CDFn)GetProcAddress(real, "CreateDXGIFactory");
    tm_log("[proxy] real CreateDXGIFactory=%p", (void *)g_real_factory);
    return TRUE;
}

#endif
