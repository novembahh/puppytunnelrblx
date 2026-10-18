# textmode

Optional headless render-reduction module for Roblox Studio dedicated
servers. Additional and disabled by default: hosters opt in through their
own launch script. Nothing here is required to run a server.

Report issues on https://github.com/novembahh/puppytunnelrblx/issues/

## Approach

One proxy DLL, `d3d11.dll`, sits next to `RobloxStudioBeta.exe` and forwards
`D3D11CreateDevice` to the real system library. After a device is created it
walks device to adapter to parent factory and patches
`IDXGIFactory::CreateSwapChain` (slot 10) in memory. Every swapchain then
gets a clamped backbuffer plus `Present` (slot 8) and `ResizeBuffers`
(slot 13) hooks. No Studio bytes are touched: no RVAs, no signatures, no
patched engine code.

Do not ship the `dxgi.dll` proxy build: under Wine a native module under
that name makes Studio die a few minutes after boot (Wine aborts on its
unimplemented `DXGID3D10CreateDevice` stub). Only `d3d11.dll` is deployed.
`src/minimal-dxgi.c` stays as a diagnostic probe, not for hosting.

Measured effect on a Wine/llvmpipe box: rasterizer worker threads drop from
roughly a full core each to near zero with the default 64 px clamp, while
frames keep presenting and the game ports stay up.

## Build

Linux cross-build (same PE artifacts run under Wine and native Windows):

```
./build-linux.sh
```

Needs `g++-mingw-w64-x86-64`. Output lands in `dist/` and is ignored by git.

Native Windows build (VS Developer Prompt):

```
build-windows.bat
```

The same PE runs under Wine and on Windows, so most hosters just grab
`textmode-d3d11.dll` from nightly instead of building.

## Host wiring (Wine/Linux)

Copy `dist/d3d11.dll` next to the Studio executable and set overrides so it
loads native, e.g. `WINEDLLOVERRIDES='d3d11=n,b;dxgi=n,b;textmode=n'`.
Remove the file to go back to stock rendering. Never copy `dxgi.dll`.

## Host wiring (native Windows)

Same single file, no overrides needed. Windows always prefers a DLL sitting
next to the exe.

1. Download `textmode-d3d11.dll` from nightly (the Textmode button on the
   main README) or build it yourself with `build-windows.bat`.
2. Rename it to `d3d11.dll` and put it in the same folder as
   `RobloxStudioBeta.exe`.
3. Start your server exactly like before. Nothing else changes.
4. Check it took: set `TEXTMODE_LOG` to a path you like before launching,
   e.g. `set TEXTMODE_LOG=C:\temp\textmode-55555.log`, then look for
   `factory hooked` and `CreateSwapChain backbuffer WxH` in the log.
5. To go back to stock, delete `d3d11.dll` from that folder.

Tuning works the same as on Wine: `TEXTMODE_MAXDIM` clamps the backbuffer
(default 64), `TEXTMODE_SKIP` forwards 1 present in N+1. Set them with `set`
before launching, e.g. `set TEXTMODE_SKIP=5`.

Heads up: the DLL is unsigned, so overeager antivirus may flag it. It only
calls stock DirectX and kernel APIs. Load-tested on native Windows and
run for hours under Wine; if Studio ever misbehaves with it, delete the
file and report it on the issues page above.

## Env vars

| Var | Default | Meaning |
|---|---|---|
| `TEXTMODE_MAXDIM` | 64 | longest backbuffer side, px (8-1024) |
| `TEXTMODE_SKIP` | 0 | forward 1 present in N+1 (0 forwards all) |
| `TEXTMODE_PASSTHROUGH` | 0 | 1 returns the real factory unhooked |
| `TEXTMODE_LOG` | temp dir | hook log path |

## Verify

The hook log shows `D3D11CreateDevice`, `QI IDXGIDevice`, `GetAdapter`,
`Adapter GetParent factory`, `factory hooked`, then
`CreateSwapChain backbuffer WxH` and running `frames=` counters. The Studio
process keeps running and its UDP game port opens as usual.
