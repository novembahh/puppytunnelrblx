#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <string.h>

static int enable_debug(void) {
    HANDLE tok = NULL;
    TOKEN_PRIVILEGES tp;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &tok))
        return 0;
    tp.PrivilegeCount = 1;
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    if (!LookupPrivilegeValueA(NULL, SE_DEBUG_NAME, &tp.Privileges[0].Luid)) {
        CloseHandle(tok);
        return 0;
    }
    AdjustTokenPrivileges(tok, FALSE, &tp, sizeof(tp), NULL, NULL);
    CloseHandle(tok);
    return GetLastError() == ERROR_SUCCESS;
}

static DWORD find_pid(const char *name) {
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    PROCESSENTRY32 pe;
    DWORD pid = 0;
    if (snap == INVALID_HANDLE_VALUE) return 0;
    pe.dwSize = sizeof(pe);
    if (Process32First(snap, &pe)) {
        do {
            if (_stricmp(pe.szExeFile, name) == 0) { pid = pe.th32ProcessID; break; }
        } while (Process32Next(snap, &pe));
    }
    CloseHandle(snap);
    return pid;
}

int main(int argc, char **argv) {
    DWORD pid;
    HANDLE proc = NULL;
    LPVOID mem = NULL;
    HANDLE th = NULL;
    size_t len;
    DWORD tid = 0;
    FARPROC loadlib;
    if (argc != 3) {
        printf("usage: tm_inject.exe <pid | process.exe> <dll path>\n");
        return 2;
    }
    pid = (DWORD)atoi(argv[1]);
    if (pid == 0) pid = find_pid(argv[1]);
    if (pid == 0) { printf("process not found: %s\n", argv[1]); return 1; }
    enable_debug();
    proc = OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION |
                       PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
                       FALSE, pid);
    if (!proc) { printf("OpenProcess(%lu) failed: %lu\n", (unsigned long)pid, GetLastError()); return 1; }
    len = (strlen(argv[2]) + 1) * sizeof(char);
    mem = VirtualAllocEx(proc, NULL, len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!mem) { printf("VirtualAllocEx failed: %lu\n", GetLastError()); CloseHandle(proc); return 1; }
    if (!WriteProcessMemory(proc, mem, argv[2], len, NULL)) {
        printf("WriteProcessMemory failed: %lu\n", GetLastError());
        CloseHandle(proc);
        return 1;
    }
    loadlib = GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA");
    if (!loadlib) { printf("no LoadLibraryA\n"); CloseHandle(proc); return 1; }
    th = CreateRemoteThread(proc, NULL, 0, (LPTHREAD_START_ROUTINE)loadlib, mem, 0, &tid);
    if (!th) { printf("CreateRemoteThread failed: %lu\n", GetLastError()); CloseHandle(proc); return 1; }
    WaitForSingleObject(th, 15000);
    printf("injected into pid %lu (tid %lu)\n", (unsigned long)pid, (unsigned long)tid);
    CloseHandle(th);
    CloseHandle(proc);
    return 0;
}
