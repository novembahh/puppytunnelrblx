# studio-textmode

cathook-style nographics extension for Roblox Studio. One PE DLL runs both
natively on Windows and under Wine on Linux.

## How it stays update-proof

No Studio bytes are touched — no RVAs, no hardcoded offsets, no version
checks. Hooks sit on stable ABI seams only:

- IAT hooks for `D3D11CreateDeviceAndSwapChain` / `CreateDXGIFactory{,1,2}`
  (import names are stable API);
- COM vtable hooks for `IDXGISwapChain::Present` (slot 8),
  `ResizeBuffers` (slot 13), `IDXGIFactory::CreateSwapChain` (slot 10)
  (COM layout is stable ABI).

Engine-level render-skip patches can be added later in `sig_targets[]`
(`src/textmode.c`, scanner already included) once located with IDA —
addresses must come with evidence, never guesses.

## What it does (no Sleep, no throttling)

- Clamps every backbuffer to `TEXTMODE_MAXDIM` px (default 64), so the
  software rasterizer has almost nothing to draw. Engine runs full speed.
- Optionally skips presents (`TEXTMODE_SKIP=599` forwards 1 in 600).

## Build

| Host | Command | Output runs on |
|---|---|---|
| Linux | `./build-linux.sh` (needs `g++-mingw-w64-x86-64`) | Wine/Linux + native Windows |
| Windows MSVC | `build-windows.bat` (VS Developer Prompt) | native Windows (+ Wine) |
| Windows MinGW | `gcc -O2 -shared -o dist\textmode.dll src\textmode.c` | both |

## Load it

Wine (server box): `WINEDLLOVERRIDES="textmode=n" WINEDLLPATH=/home/sex/textmode/dist ... wine Studio...`

Native Windows: `tm_inject.exe RobloxStudioBeta <full path>\textmode.dll`

## Env vars

| Var | Default | Meaning |
|---|---|---|
| `TEXTMODE` | on | `0` = stay dormant |
| `TEXTMODE_MAXDIM` | 64 | longest backbuffer side, px (8–1024) |
| `TEXTMODE_SKIP` | 0 | forward 1 present in N+1 (0 = all) |
| `TEXTMODE_LOG` | temp dir | hook log path |

## Verify

Hook log shows `D3D11CreateDeviceAndSwapChain backbuffer WxH`, `swapchain
hooked`, `clamp backbuffer ...`. Rendering CPU should collapse while the
Studio process keeps running and (once logged in) opens its UDP game port.
