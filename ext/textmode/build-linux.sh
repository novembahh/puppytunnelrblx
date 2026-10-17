#!/bin/bash
# Cross-build the extension on Linux for BOTH Linux/Wine and native Windows
# (same PE artifacts run in both). Needs: sudo apt install g++-mingw-w64-x86-64
set -e
cd "$(dirname "$0")"
CC="${CC:-x86_64-w64-mingw32-gcc}"
command -v "$CC" >/dev/null || { echo "missing $CC (sudo apt install g++-mingw-w64-x86-64)"; exit 1; }
mkdir -p dist
"$CC" -O2 -Wall -shared -o dist/textmode.dll src/textmode.c -Wl,--subsystem,windows
"$CC" -O2 -Wall -o dist/tm_inject.exe src/tm_inject.c
"$CC" -O2 -Wall -shared -o dist/d3d11.dll src/proxy.c -DPROXY_D3D11 src/proxy-d3d11.def -Wl,--subsystem,windows
"$CC" -O2 -Wall -shared -o dist/dxgi.dll src/proxy.c -DPROXY_DXGI src/proxy-dxgi.def -Wl,--subsystem,windows
sha256sum dist/d3d11.dll dist/dxgi.dll
sha256sum dist/textmode.dll dist/tm_inject.exe
echo OK
