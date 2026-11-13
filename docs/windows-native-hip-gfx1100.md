# Native Windows HIP (ROCm 10.1) — gfx1100 validation

This is an **opt-in** inference path for Radeon RX 7900 XTX on native Windows.
It uses `yue2.cpp` + GGML HIP, not Python or a Vulkan translation layer.
Explicit `HIP` selection uses `--device ROCm0` and must fail rather than
silently move inference to a different backend.

## Source provenance and upstream dependencies

Until the engine and GGML changes are merged, keep the published engine pin
in `engines/yue2-cpp-source.json` unchanged. For integration testing, use
these exact temporary source revisions:

| Component | Revision | Purpose |
| --- | --- | --- |
| GGML base (engine submodule) | `f3fbd7a8c89157ff376d44638f60c4bef792d0a2` | baseline used for physical validation |
| GGML HIP fixes | `AIwork4me/ggml@39927eae359a6a57118c179e63287d699627fb20` | Windows hipcc FA_QUANTS + unused Windows header |
| Native engine (physically tested baseline) | `AIwork4me/yue2.cpp@d9500629ab43c5d2eb66c032d874e7e5a26662cb` | HIP backend build and MP3 link fix |
| Studio HIP integration (baseline) | `AIwork4me/YuE2-Studio@2107ef6602355a7e97d806f4347ab0e9ad0c6063` | backend selection, UI and package copying |

New hardening commits must be retested before replacing those verified
revisions in a performance or compatibility claim. In particular the Studio
release script builds **its pinned engine**, not automatically these fork
heads. Changing a Git SHA is not an acceptable substitute for a physical test.

The `yue2.cpp` upstream PR should target **`timoncool:companion`**.
A PR against `master` would pull in unrelated companion history. The GGML
fixes should be submitted to the maintainers of the fork that contains the
pinned baseline, then the merged submodule SHA can be recorded in yue2.cpp.
After the engine merge, update the Studio engine pin separately.

## Reproduce the native engine before Studio integration

On Windows x64 with RX 7900 XTX, the official ROCm 10.1 gfx110X SDK,
CMake, Ninja and MSVC C++ Build Tools:

```powershell
git clone --recurse-submodules https://github.com/AIwork4me/yue2.cpp.git
cd yue2.cpp
git checkout --detach d9500629ab43c5d2eb66c032d874e7e5a26662cb
git submodule update --init --recursive
git -C ggml fetch https://github.com/AIwork4me/ggml.git 39927eae359a6a57118c179e63287d699627fb20
git -C ggml checkout --detach 39927eae359a6a57118c179e63287d699627fb20

$env:ROCM_PATH = 'C:\TheRock\build'   # adjust to the installed ROCm 10.1 SDK
$env:HIP_PATH = $env:ROCM_PATH
cmd /c buildhip.cmd
```

Use the build script in the selected engine revision, or the separate
portability follow-up when available. Save the actual `git rev-parse HEAD`
of both repositories and the binary hashes: the GGML submodule is deliberately
dirty relative to the original pin during pre-merge validation.

Check that `build-hip/yue-server.exe` and `build-hip/ggml-hip.dll` exist.
Verify with a real `--device ROCm0` startup and a full AR -> NAR -> VAE
song, not just a GPU enumeration message. Verify runtime DLLs and device
kernel packages come from the same ROCm 10.1 distribution.

## Exercise Studio without silently changing the production pin

Build Studio from the HIP integration branch. Use an **isolated HIP engine
bundle** containing the freshly built `yue-server.exe`, `ggml-hip.dll`,
required ROCm 10.1 runtime DLLs, `amd_comgr.dll`, `rocm_kpack.dll`,
hipBLAS/rocBLAS dependencies and the corresponding kernel data. Do not put
model weights or runtime libraries into Git history.

```powershell
# Run in the environment of the new Studio executable.
$env:YUE_ENGINE_ROOT = 'C:\dev\yue2-hip-nightly\artifacts\yue2-hip-portable-flat'
Remove-Item Env:\ROCM_PATH, Env:\HIP_PATH -ErrorAction SilentlyContinue
```

The engine root override is for integration testing only. For a product
install, use the application's `resources/yue2-cpp` directory and verify
that `ggml-hip.dll` is present. The bundle preflight checks app-local DLLs
without assuming an installed SDK. Startup still needs to confirm that
`ROCm0` is a real usable device. Choose **HIP explicitly**, not Auto,
when collecting HIP-only evidence.

Perform an actual Tauri GUI test: select HIP; create a song; verify the
job, progress, cancellation, local library, playback and MP3/WAV export.
The standalone yue-server health + job contract does **not** constitute a
GUI end-to-end test.

## Tests and failure policy

1. Native HIP C++ and hipBLAS smoke tests on actual gfx1100.
2. GGML HIP numerics and AR/NAR/VAE, Q8_0 and BF16, full song and replay.
3. `cargo test -p music-server --locked` on Windows and Linux, plus
   `npm --prefix app test`, TypeScript and production frontend build.
4. Windows packaged engine dependency-closure test, including negative tests
   with `ggml-hip.dll` or `amd_comgr.dll` removed from a *copy* of the bundle.
5. Tauri GUI full-song integration, in a clean environment with no SDK
   environment variables. Collect log, audio, GPU telemetry and SHA256.
6. A clean-install portable package test without an SDK, using only properly
   licensed distributable files.

A missing HIP device, library, kernel or numerical failure is a **FAIL**,
not a pass via another inference backend. Do not relax Windows security
policies to hide Smart App Control failures. Report test statuses as
PASS / FAIL / BLOCKED / NOT RUN and identify the exact tested HEAD.

The original overnight evidence is on a real RX 7900 XTX, but the hardening
branches and final Tauri GUI have not been physically retested in this
repository. Do not claim all Radeon families are supported: `gfx1100`
is the only physically verified architecture.
