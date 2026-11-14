# Native Windows HIP (ROCm 10.1) — architecture support and gfx1100 validation

This is an **opt-in** inference path for AMD Radeon on native Windows. It uses
`yue2.cpp` + GGML HIP, not Python or a Vulkan translation layer. Explicit
`HIP` selection uses `--device ROCm0` and must fail rather than silently move
inference to a different backend.

`gfx1100` (Radeon RX 7900 XTX) is the **physically verified reference
architecture** — not the product boundary. The build, staging and release
plumbing are target-parameterized (`HIP_ARCHS` / `-HipArch`), and adding a
family is a build + validation task, not a code change.

## Architecture support matrix

Evidence levels, honestly separated: **Verified** = full AR/NAR/VAE song
generation on physical hardware through the Studio GUI; **Build-verified** =
device code compiles and the runtime closure exists, no physical run;
**Experimental** = compiles, but no complete runtime closure available from
the SDKs used here; **Unsupported** = no path today.

| Family | gfx ids | Example cards | TheRock 10.1 dist | Device compile | BLAS kernel closure | Physical validation | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| RDNA 3 dGPU | `gfx1100` | RX 7900 XTX | `gfx110X` | PASS (full build) | present (gfx1100-1103) | 33-check regression + GUI + installer payload (actual NSIS installation remains BLOCKED) | **Verified** |
| RDNA 3 dGPU | `gfx1101`, `gfx1102` | RX 7800 XT / 7700 XT / 7600 | `gfx110X` | PASS (probe + family build) | present | none | **Build-verified** |
| RDNA 3 iGPU | `gfx1103` | Phoenix APUs | `gfx110X` | PASS (probe) | present | none | **Build-verified** (shared-memory VRAM may gate model size) |
| RDNA 3+ APU | `gfx1150`, `gfx1151` | Ryzen 890M / Radeon 8060S | none for 10.1 | PASS (probe; device libs are arch-generic) | **absent** from every dist used here | none | **Experimental** |
| RDNA 2 | `gfx1030`/`1031`/`1032` | RX 6800 / 6700 / 6600 | `gfx103X` | PASS (probe) | needs the `gfx103X` dist's closure | none | **Experimental** |
| RDNA 4 | `gfx1200`, `gfx1201` | RX 9070 series | `gfx120X` | PASS (probe) | needs the `gfx120X` dist's closure | none | **Experimental** |

Facts behind the table (measured, not assumed):

- hipcc from the `gfx110X` TheRock dist (clang 24) **compiles device code for
  all eleven targets above** — the LLVM device-library bitcode is
  arch-generic (`qa/arch-probe`, 2026-11-14 session).
- The **runtime closure is family-scoped**: this dist's rocBLAS/hipBLASLt
  kernels and `.kpack` files cover gfx1100-1103 only. A multi-target
  `ggml-hip.dll` without matching BLAS kernels fails at runtime, so a new
  family needs its own dist (`gfx103X`, `gfx120X` both exist on AMD's stable
  repo for 10.1), or an official HIP SDK install that ships its kernels.
- The engine reports the **actual** device target from HIP device properties
  (`gcnArchName`) at startup; nothing selects a backend from a marketing-name
  table. `tests/test-hip-windows.ps1` pins this: the enumerated arch must
  equal the expected target or the run FAILs.

## Universal packaging design

Two designs were evaluated for "one Studio, many Radeons":

**A. One multi-family package** — a multi-target `ggml-hip.dll` plus a
file-union of every family's rocBLAS/hipBLASLt kernel directories and
`.kpack`s. One installer, zero user choices; but the union of kernel
closures is untested, per-family dist versions can drift, and the download
grows ~336 MB per family (the verified gfx1100 bundle is 336 MB).
CI builds and validation multiply per family, and one family's bad kernels
can break the only package everyone downloads.

**B. Architecture-specific HIP engine packages** (chosen) — the *same*
Studio application, with a per-family engine bundle staged by
`scripts/build-yue-runtime.ps1 -RuntimeBackend hip -HipArch <ids>` and
packaged by `scripts/build-release.ps1` into flavor-tagged artifacts
(`…-hip-<targets>_x64-setup.exe`, `latest-hip-<targets>.json`). Release
assets can be presented by **card generation**, so users need not learn
targets or swap DLLs. A proposed `RDNA 3` package can contain device code
for RX 7900/7800/7700/7600, but **only gfx1100/RX 7900 XTX is physically
validated** so far; a retail RDNA3-wide compatibility claim requires
runtime + full-song testing on the other targets. `gfx103X`/`gfx120X`
packages follow the same pipeline only after their own kernel closure and
physical acceptance are completed. The default CUDA/Vulkan Windows package
is untouched, and the HIP updater manifest is tag-named so no channel can
ever cross-serve the other flavor.

Design B is what the gfx1100 physical evidence covers end-to-end; A remains
a possible follow-up (a union-closure experiment), not a claim. Both share
the critical property the mission requires: **adding another compatible
architecture changes build inputs and test coverage, not Studio code** —
detection is layout-based, the engine validates the real device, and every
artifact already carries its target list.

## Windows driver/runtime deployment and release-signing policy

**Physical test status is not ISV deployment approval.** The current
gfx1100 reference run used an isolated app-local TheRock ROCm 10.1 bundle,
including `amdhip64*.dll`. Keep that setup for reproducible development
and regression; it is NOT yet the approved production installer layout.

AMD's Windows HIP SDK *application deployment guidelines* state that
production users obtain the HIP **runtime from the AMD GPU driver**, while
ISVs may ship the relevant **HIP SDK libraries** with their applications.
The minimum required GPU driver and its compatible HIP runtime should be
documented together with the compiler/library version. The current local
TheRock 10.1 DLL closure has not yet been independently qualified against
that driver-runtime deployment policy. A public release must pass a clean
Windows install using the intended driver/runtime arrangement; do not claim
"no SDK required" based solely on the private runtime validation.

Reference:
https://rocm.docs.amd.com/projects/install-on-windows/en/develop/conceptual/deployment-guidelines.html

Before the first official Windows HIP release:
1. Check each AMD redistributable DLL, kernel data package and licence for
   the chosen Windows SDK/driver pairing; record the source and SPDX licence.
2. Validate HIP device enumeration, `ROCm0`, GGML backend dynamic loading,
   rocBLAS kernels and one entire song with the AMD-driver-provided runtime.
   If the certified combination needs a private-runtime exception, obtain
   explicit owner/maintainer approval and document its basis and limitations.
3. Sign the first-party HIP engine DLLs and `yue-server.exe` **before**
   Tauri bundles resources. Configure Tauri Windows Authenticode signing
   for the Studio executable and NSIS installer **during** its build.
   The Tauri updater Ed25519 `.sig` must be generated over the finalized
   installer; **never re-sign or modify the installer after that step**.
4. For an opt-in publisher-signed HIP release, set a valid
   `YUE_AUTHENTICODE_CERT_THUMBPRINT` and `YUE_REQUIRE_SIGNED_RELEASE=1`
   on the trusted Windows release machine, then use `build-release.ps1`.
   `scripts/audit-hip-release.ps1` checks HIP artifact isolation,
   manifest-to-installer naming, architecture stamp, and Authenticode state.
   It does **not** cryptographically validate the independent Tauri updater
   Ed25519 signature; verify that separately with Tauri's updater key.
5. OV and EV certificates have the same SmartScreen reputation-building
   behavior (EV no longer grants immediate initial trust). Smart App
   Control and SmartScreen are distinct checks: a signature is necessary
   for a trustworthy public distribution but is not a guarantee against
   every warning or block. Do not disable SAC to pass the release gate.

Microsoft:
https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options
https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation

Tauri:
https://v2.tauri.app/distribute/sign/windows/

**HIP update isolation:** the Tauri release config must embed the
`latest-hip-<targets>.json` endpoint, matching the generated manifest.
The HIP manifest and all corresponding installer/ZIP assets must coexist
on the upstream release that GitHub marks as `latest`; otherwise
`releases/latest/download/` will resolve to a release without those
assets. The maintainer owns this publication policy. A different
manifest filename alone never separated the updater before the fix.

**Support tiers:** `gfx1100` (RX 7900 XTX) remains physical PASS on the
recorded code/build; `gfx1101`–`gfx1103` remain compile/closure-only
until separately run; `gfx103X`, `gfx115X`, `gfx120X` have no
end-to-end physical evidence for this project. In particular the mere
existence of a kernel tarball, a successful hipcc compile or a signature
does not qualify a GPU family for release.

## Source provenance and upstream dependencies

Until the engine and GGML changes are merged, keep the published engine pin
in `engines/yue2-cpp-source.json` unchanged. For integration testing, use
these exact temporary source revisions:

| Component | Revision | Purpose |
| --- | --- | --- |
| GGML base (engine submodule) | `f3fbd7a8c89157ff376d44638f60c4bef792d0a2` | baseline used for physical validation (`timoncool/ggml:fattn-mask-broadcast-0.29`, head of ServeurpersoCom/ggml PR #4) |
| GGML HIP fixes | `AIwork4me/ggml@39927eae359a6a57118c179e63287d699627fb20` (PR path: `openmaster-ai/ggml:fix/hip-windows-fa-quants-stringify`) | Windows hipcc FA_QUANTS + unused Windows header |
| Native engine (physically tested baseline) | `AIwork4me/yue2.cpp@df4b193755d4dc5648807763dfc6e27b56458ab1` | HIP backend build, portability, libomp-free closure |
| Studio HIP integration | `fix/native-hip-pr-readiness` on the fork | backend selection, UI, arch-parameterized packaging |

New hardening commits must be retested before replacing those verified
revisions in a performance or compatibility claim. In particular the Studio
release script builds **its pinned engine**, not automatically these fork
heads. Changing a Git SHA is not an acceptable substitute for a physical test.

The `yue2.cpp` upstream PR targets **`timoncool:companion`**. A PR against
`master` would pull in unrelated companion history. The GGML fixes go to
`timoncool/ggml:fattn-mask-broadcast-0.29` (the PR #4 head), so they flow
into PR #4 when that branch merges; the merged submodule SHA is then
recorded in yue2.cpp. After the engine merge, update the Studio engine pin
separately.

## Reproduce the native engine before Studio integration

On Windows x64 with a Radeon, the ROCm 10.1 SDK for its family
(e.g. the TheRock `gfx110X` dist), CMake, Ninja and MSVC C++ Build Tools:

```powershell
git clone --recurse-submodules https://github.com/AIwork4me/yue2.cpp.git
cd yue2.cpp
git checkout --detach df4b193755d4dc5648807763dfc6e27b56458ab1
git submodule update --init --recursive
git -C ggml fetch https://github.com/AIwork4me/ggml.git 39927eae359a6a57118c179e63287d699627fb20
git -C ggml checkout --detach 39927eae359a6a57118c179e63287d699627fb20

$env:ROCM_PATH = 'C:\TheRock\build'   # adjust to the installed ROCm 10.1 SDK
$env:HIP_PATH = $env:ROCM_PATH
$env:HIP_ARCHS = 'gfx1100'            # or a comma list within one dist family
cmd /c buildhip.cmd
```

Check that `build-hip/yue-server.exe` and `build-hip/ggml-hip.dll` exist.
Verify with a real `--device ROCm0` startup and a full AR -> NAR -> VAE
song, not just a GPU enumeration message. Verify runtime DLLs and device
kernel packages come from the same ROCm 10.1 distribution. The engine
acceptance harness (`tests/test-hip-windows.ps1` in the engine repo) runs
all of that, including the fail-closed negative cases.

## Exercise Studio without silently changing the production pin

Build Studio from the HIP integration branch. Use an **isolated HIP engine
bundle** containing the freshly built `yue-server.exe`, `ggml-hip.dll`,
required ROCm 10.1 runtime DLLs, `amd_comgr.dll`, `rocm_kpack.dll`,
hipBLAS/rocBLAS dependencies and the corresponding kernel data. Do not put
model weights or runtime libraries into Git history.

```powershell
# Run in the environment of the new Studio executable.
$env:YUE_ENGINE_ROOT = 'C:\bundles\yue2-hip-gfx1100'
Remove-Item Env:\ROCM_PATH, Env:\HIP_PATH -ErrorAction SilentlyContinue
```

The engine root override is for integration testing only. For a product
install, use the application's `resources/yue2-cpp` directory and verify
that `ggml-hip.dll` is present. The bundle preflight checks app-local DLLs
without assuming an installed SDK. Startup still needs to confirm that
`ROCm0` is a real usable device. Choose **HIP explicitly**, not Auto,
when collecting HIP-only evidence.

Perform an actual Tauri GUI test: select HIP; create a song; verify the
job, progress, cancellation, local library, playback and FLAC export.
Other export formats need separate tests before a PASS claim.
The standalone yue-server health + job contract does **not** constitute a
GUI end-to-end test.

## Tests and failure policy

1. Native HIP C++ and hipBLAS smoke tests on actual hardware of the family.
2. GGML HIP numerics and AR/NAR/VAE, Q8_0 and BF16, full song and replay
   (`tests/test-hip-windows.ps1`).
3. `cargo test -p music-server --locked` on Windows and Linux, plus
   `npm --prefix app test`, TypeScript and production frontend build.
4. Windows packaged engine dependency-closure test, including negative tests
   with `ggml-hip.dll`, `amd_comgr.dll`, the HIP runtime or the rocBLAS
   kernel library removed from a *copy* of the bundle, and an unknown
   `--device` id.
5. Tauri GUI full-song integration, in a clean environment with no SDK
   environment variables. Collect log, audio, GPU telemetry and SHA256.
6. A clean-install portable package test without an SDK, using only properly
   licensed distributable files.

A missing HIP device, library, kernel or numerical failure is a **FAIL**,
not a pass via another inference backend. Do not relax Windows security
policies to hide Smart App Control failures. Report test statuses as
PASS / FAIL / BLOCKED / NOT RUN and identify the exact tested HEAD.

Physical evidence today covers `gfx1100` only. Nothing in this repository
physically validates other families: build-verified targets need a real
card's run through the same harness before any "supported" claim, and
experimental targets additionally need a runtime closure. Per-family model
memory must be measured on the real GPU — do not assume every card fits the
same model sizes as a 24 GB RX 7900 XTX.

Ancillary Studio tools (HOT-Step, MIDI, trainer, stem separator, karaoke,
assistant) keep their existing compute backends; only the yue2 music engine
gains native HIP on Windows.
