#include <windows.h>
#include <stdio.h>
typedef HRESULT (WINAPI *CDFn)(REFIID, void **);
typedef HRESULT (WINAPI *CDFn2)(UINT, REFIID, void **);
static HMODULE g_real = NULL;
static CDFn g_p0 = NULL;
static CDFn g_p1 = NULL;
static CDFn2 g_p2 = NULL;
static HMODULE lazy_real(void) {
    char sys[MAX_PATH];
    char full[MAX_PATH * 2];
    HMODULE h;
    if (g_real) return g_real;
    if (!GetSystemDirectoryA(sys, sizeof(sys))) return NULL;
    snprintf(full, sizeof(full), "%s\\dxgi.dll", sys);
    h = LoadLibraryA(full);
    if (!h) return NULL;
    g_p0 = (CDFn)GetProcAddress(h, "CreateDXGIFactory");
    g_p1 = (CDFn)GetProcAddress(h, "CreateDXGIFactory1");
    g_p2 = (CDFn2)GetProcAddress(h, "CreateDXGIFactory2");
    g_real = h;
    return h;
}
HRESULT WINAPI CreateDXGIFactory(REFIID r, void **f) {
    if (!lazy_real() || !g_p0) return E_FAIL;
    return g_p0(r, f);
}
HRESULT WINAPI CreateDXGIFactory1(REFIID r, void **f) {
    if (!lazy_real() || !g_p1) return E_FAIL;
    return g_p1(r, f);
}
HRESULT WINAPI CreateDXGIFactory2(UINT fl, REFIID r, void **f) {
    if (!lazy_real() || !g_p2) return E_FAIL;
    return g_p2(fl, r, f);
}
BOOL APIENTRY DllMain(HMODULE m, DWORD r, LPVOID x) {
    (void)x;
    if (r == DLL_PROCESS_ATTACH) DisableThreadLibraryCalls(m);
    return TRUE;
}
