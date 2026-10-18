@echo off
setlocal
if not exist dist mkdir dist
where cl >nul 2>nul
if errorlevel 1 (
  echo MSVC cl.exe not found. Use a VS Developer Prompt, or MinGW:
  echo   gcc -O2 -shared -o dist\textmode.dll src\textmode.c
  echo   gcc -O2 -o dist\tm_inject.exe src\tm_inject.c
  exit /b 1
)
cl /nologo /O2 /LD src\textmode.c /link /OUT:dist\textmode.dll
cl /nologo /O2 src\tm_inject.c /link /OUT:dist\tm_inject.exe
cl /nologo /O2 /LD src\proxy.c /DPROXY_D3D11 /link /DEF:src\proxy-d3d11.def /OUT:dist\d3d11.dll
echo OK
