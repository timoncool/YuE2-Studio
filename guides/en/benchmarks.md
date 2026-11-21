# Benchmarks and measurements

[Back to YuE2 Studio](../../README.md)

## How fast is it

### Engine benchmarks

Historical measurements from the 4.0.0 development builds on Windows 11, Ryzen 9 9950X3D and RTX 4090 24 GB with CUDA 13:

| Model and engine | Workload | Wall time | Audio seconds per second | Peak GPU memory |
| --- | --- | ---: | ---: | ---: |
| YuE2 · yue2.cpp · Q8_0 | 60 s song, Flash Attention | 20.0 s | 3.0× | 5,045 MiB |
| YuE2 · yue2.cpp · Q8_0 | 240 s song, Flash Attention | 54.4 s | 4.4× | 6,500 MiB |
| MiniMax Music3 · minimaxmusic.cpp · Q8_0 | 30 s song, 30 steps, CFG 1.5 | 33.0 s | 0.9× | — |

ACE-Step's native 2B turbo Q8_0 took 21.5 ms per DiT step on warm runs. Its roughly 8.1 GiB peak occurred during VAE decoding. This timing covers one stage of synthesis. A dash indicates an unrecorded measurement.

**YuE2, native yue2.cpp `892b9b0`.** Windows 11 (build 26100), Ryzen 9 9950X3D and RTX 4090 24 GB (driver 617.42). All runs used YuE2 3B Q8_0, the F32 VAE, NAR companion v9 and seed 1221. Replay used 100 fixed semantic tokens and two synthesis steps. Text to song started from style and lyrics and used 32 synthesis steps.

| Workload | Backend | Start to first WAV (s) | Warm median (s) | Peak GPU (GiB) | Above GPU baseline (GiB) | Peak process-tree RSS (GiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Semantic replay (3.999 s) | CUDA 13 · cuBLAS 13.10.0.20 | 6.516 | 0.172 | 6.88 | 4.51 | 3.40 |
| Semantic replay (3.999 s) | CUDA 12 · cuBLAS 12.9.2.10 | 4.969 | 0.125 | 6.88 | 4.50 | 3.45 |
| Semantic replay (3.999 s) | Vulkan | 13.766 | 0.252 | 6.54 | 4.17 | 3.82 |
| Semantic replay (3.999 s) | CPU | 16.234 | 3.503 | — | — | 6.41 |
| Text to song (29.999 s) | CUDA 13 · cuBLAS 13.10.0.20 | 8.813 | 4.759 | 8.13 | 5.75 | 3.37 |
| Text to song (29.999 s) | Vulkan | 16.609 | 5.861 | 8.19 | 5.81 | 3.08 |

Cold time includes server startup and the first request through receipt of the encoded WAV. Warm time is the median of three requests with the models kept loaded. Memory was sampled every 200 ms during startup and the first request. GPU peak is for the whole RTX 4090. The increase uses the reading before server launch as the baseline. CPU runs report process-tree RSS alone.

**ACE-Step Studio 4.0.2, native acestep.cpp `9eb75e8`.** On Windows with an RTX 4090 (driver 617.42), each run generated 30 seconds of instrumental audio using Turbo Q8_0, eight Euler steps and seed 42.

| Backend | Start server to first WAV | Warm request (median of 3) | Audio seconds per second, warm | Peak GPU memory (whole card) | Peak process-tree RAM |
| --- | ---: | ---: | ---: | ---: | ---: |
| CUDA 13 | 6.813 s | 0.474 s | 63.3× | 12.00 GiB | 1.97 GiB |
| Vulkan | 19.063 s | 0.507 s | 59.2× | 12.42 GiB | 2.66 GiB |

Memory was sampled every 200 ms. Peak GPU usage rose 9.56 GiB on CUDA and 9.99 GiB on Vulkan above the desktop baseline. Windows file caching was enabled. These measurements use the published ACE-Step Studio engine.

For seed 42, both WAVs contain 30.0 s of stereo audio at 48 kHz. Measured in 100 ms windows across both channels, the last 5.3 s on CUDA and 5.0 s on Vulkan stay below −60 dBFS RMS. The speed column uses the full WAV duration.

<!-- music-gpu-matrix:start -->
### Music engine GPU matrix · 5.0 development and published builds

Windows 11, RTX 4090, driver 617.42. Cold includes server launch and the first WAV response. Warm is the median of three requests to the same server. Memory peaks are cold-run whole-card VRAM and process-tree RSS. Warm pre-request VRAM is the median whole-card reading immediately before each of the three warm requests. Resident WDDM VRAM and RSS are medians of three idle process-tree samples after one warm-up on each listed CUDA 13 build. Compare rows within each workload and check the weight profiles, silence and exact build before drawing a speed conclusion.

| Model and engine | Backend | WAV (s) | Cold (s) | Warm (s) | Warm audio s/s | VRAM peak / Δ (GiB) | Warm pre-request VRAM (GiB) | Peak RAM (GiB) | Resident WDDM VRAM / RSS (GiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 30 s · 32 steps | CUDA 13 | 29.999 | 9.735 | 5.166 | 5.81× | 7.67 / +5.74 | 7.67 | 3.36 | — |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 30 s · 32 steps | CUDA 12 | 29.999 | 11.219 | 5.155 | 5.82× | 7.64 / +5.75 | 7.63 | 3.35 | — |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 30 s · 32 steps | Vulkan | 29.999 | 16.609 | 5.861 | 5.12× | 8.19 / +5.81 | 8.87 | 3.08 | — |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 30 s · 32 steps | CUDA 13 | 29.999 | 16.807 | 7.441 | 4.03× | 7.02 / +4.68 | 3.16 | 6.24 | — |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 30 s · 32 steps | CUDA 12 | 29.999 | 19.546 | 7.957 | 3.77× | 6.74 / +4.65 | 2.90 | 6.17 | — |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 30 s · 32 steps | Vulkan | 29.999 | 28.258 | 10.286 | 2.92× | 6.44 / +4.51 | 2.54 | 6.85 | — |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 13 | 3.999 | 6.062 | 0.140 | 28.52× | 6.44 / +4.51 | 6.44 | 3.35 | 4.50 / 0.77 |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 13 | 3.999 | 7.482 | 0.383 | 10.44× | 6.28 / +3.92 | 3.03 | 5.84 | — |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 12 | 3.999 | 4.312 | 0.110 | 36.20× | 6.43 / +4.50 | 6.43 | 3.34 | — |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 12 | 3.999 | 7.869 | 0.420 | 9.52× | 6.46 / +4.10 | 3.03 | 5.76 | — |
| YuE2 · yue2.cpp `892b9b0` · 3B Q8_0 + F32 VAE; NAR v9 · 100 tokens · 2 steps | Vulkan | 3.999 | 13.766 | 0.252 | 15.88× | 6.54 / +4.17 | 6.54 | 3.82 | — |
| YuE2 · audio.cpp `41242418` · 3B Q8_0 + F16 VAE; NAR v9 · 100 tokens · 2 steps | Vulkan | 3.999 | 8.373 | 0.974 | 4.11× | 6.31 / +3.94 | 2.89 | 5.33 | — |
| ACE-Step 1.5 · acestep.cpp `d058cd8` · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | CUDA 13 | 30.000 | 4.046 | 0.457 | 65.59× | 9.87 / +7.79 | 9.89 | 2.14 | 7.80 / 0.92 |
| ACE-Step 1.5 · acestep.cpp `d058cd8` · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | CUDA 12 | 30.000 | 7.516 | 0.409 | 73.40× | 9.87 / +7.80 | 9.89 | 2.07 | — |
| ACE-Step 1.5 · acestep.cpp `d058cd8` · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | Vulkan | 30.000 | 4.844 | 0.434 | 69.13× | 8.72 / +6.62 | 8.72 | 1.32 | — |
| ACE-Step 1.5 · audio.cpp `41242418` · Turbo Q8_0, packed GGUF · 30 s · 8 Euler steps | CUDA 13 | 30.000 | 10.512 | 1.387 | 21.62× | 10.43 / +8.49 | 10.48 | 2.58 | — |
| ACE-Step 1.5 · audio.cpp `41242418` · Turbo Q8_0, packed GGUF · 30 s · 8 Euler steps | CUDA 12 | 30.000 | 11.398 | 1.536 | 19.53× | 10.57 / +8.50 | 10.62 | 2.67 | — |
| ACE-Step 1.5 · audio.cpp `41242418` · Turbo Q8_0, packed GGUF · 30 s · 8 Euler steps | Vulkan | 30.000 | 21.746 | 2.237 | 13.41× | 10.23 / +8.30 | 10.22 | 4.60 | — |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Minimal Q3_K_M / Q4_K_M + F32 vocoder · 3.2 s · 2 steps | CUDA 13 | 3.193 | 8.391 | 1.116 | 2.86× | 9.33 / +7.26 | 9.33 | 4.43 | 7.25 / 1.00 |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Minimal Q3_K_M / Q4_K_M + F32 vocoder · 3.2 s · 2 steps | CUDA 12 | 3.193 | 6.156 | 1.119 | 2.85× | 9.33 / +7.25 | 9.33 | 4.53 | — |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | CUDA 13 | 3.193 | 13.766 | 1.769 | 1.80× | 15.21 / +13.14 | 15.21 | 8.85 | — |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | CUDA 12 | 3.193 | 8.078 | 1.730 | 1.85× | 15.18 / +13.10 | 15.18 | 8.60 | — |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Minimal Q3_K_M / Q4_K_M + F32 vocoder · 3.2 s · 2 steps | Vulkan | 3.193 | 6.093 | 1.512 | 2.11× | 8.97 / +6.89 | 8.97 | 4.38 | — |
| MiniMax Music3 · minimaxmusic.cpp `46c4c90` · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | Vulkan | 3.193 | 9.531 | 1.922 | 1.66× | 14.82 / +12.75 | 14.83 | 8.26 | — |
| MiniMax Music3 Studio v4.0.3 · minimaxmusic.cpp `f476b0e` · Minimal Q3_K_M / Q4_K_M + F32 · 3.2 s · 2 steps | CUDA 13 | 3.193 | 7.312 | 1.257 | 2.54× | 9.32 / +7.38 | 9.32 | 4.56 | 7.39 / 0.89 |
| MiniMax Music3 · minimaxmusic.cpp `f476b0e` · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | CUDA 13 | 3.193 | 9.156 | 1.852 | 1.72× | 15.18 / +13.23 | 15.18 | 8.91 | — |
| MiniMax Music3 · minimaxmusic.cpp `f476b0e` · Minimal Q3_K_M / Q4_K_M + F32 · 3.2 s · 2 steps | Vulkan | 3.193 | 7.343 | 1.987 | 1.61× | 8.96 / +7.03 | 8.96 | 4.33 | — |
| MiniMax Music3 · minimaxmusic.cpp `f476b0e` · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | Vulkan | 3.193 | 16.047 | 2.295 | 1.39× | 14.94 / +13.00 | 14.94 | 8.35 | — |
| MiniMax Music3 · audio.cpp `41242418` · LM/depth/DiT Q8_0 · 3.2 s · 2 steps | CUDA 13 | 3.193 | 14.604 | 8.720 | 0.37× | 10.97 / +9.04 | 2.47 | 8.87 | — |
| MiniMax Music3 · audio.cpp `41242418` · LM/depth/DiT Q8_0 · 3.2 s · 2 steps | CUDA 12 | 3.193 | 14.597 | 8.436 | 0.38× | 11.12 / +8.98 | 2.59 | 8.68 | — |
| MiniMax Music3 · audio.cpp `41242418` · LM/depth/DiT Q8_0 · 3.2 s · 2 steps | Vulkan | 3.193 | 17.621 | 10.478 | 0.30× | 10.65 / +8.72 | 2.39 | 8.66 | — |
| Stable Audio 3 · HOT-Step `cdef1b0d67af` · Medium BF16/F16 · 4 s · 4 steps | CUDA 12.8 | 4.000 | 6.737 | 0.254 | 15.72× | 9.30 / +7.37 | 9.30 | 2.82 | — |
| Stable Audio 3 · audio.cpp `41242418` · Small Music Q8_0 · 4 s · 4 steps | CUDA 12 | 4.000 | 1.259 | 0.078 | 51.18× | 3.54 / +1.60 | 3.55 | 1.17 | — |
| Stable Audio 3 · audio.cpp `41242418` · Small Music Q8_0 · 4 s · 4 steps | CUDA 13 | 4.000 | 1.893 | 0.071 | 56.00× | 3.53 / +1.60 | 3.54 | 1.18 | — |
| YuE2 Studio v4.1.1 · yue2.cpp `67f59c1` · 3B Q8_0 + F32 VAE · 100 tokens · 2 steps | CUDA 13 | 3.999 | — | — | — | — | — | — | 4.61 / 0.65 |
| ACE-Step Studio v4.0.2 · acestep.cpp `9eb75e8` · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | CUDA 13 | 30.000 | — | — | — | — | — | — | 9.51 / 0.78 |

Each workload has its own fixed input and step count. YuE2 VAE precision differs, and the audio.cpp text-to-song request fixes the semantic-token budget. The native ACE CUDA take ends with five quiet seconds. MiniMax Quality and audio.cpp Q8_0 share four weights and use different vocoders. Stable Audio 3 compares Medium BF16 to Small Q8_0. The ACE-Step d058cd8 and MiniMax 46c4c90 rows use the exact separately published Windows 5.0 runtime pins. The WSL2 table identifies the measured Linux pins. MiniMax f476b0e is the 4.0.3 control.

[Individual runs, residency, SHA-256, exact weights and output checks](../../docs/music-gpu-benchmarks.json)
<!-- music-gpu-matrix:end -->

<!-- music-gpu-wsl:start -->
#### Native music engines · Linux in WSL2

RTX 4090, driver 617.42. The pinned Linux executables ran in WSL2 with weights on a Windows drive. Cold includes process launch through receipt of the first WAV. Warm is the median of three requests to the same process. Memory peaks cover whole-card VRAM and Linux process-tree RSS.

| Model and engine | Backend | WAV (s) | Cold (s) | Warm (s) | VRAM peak / Δ (GiB) | Peak RAM (GiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| YuE2 · yue2.cpp 892b9b0 · 3B Q8_0 + F32 VAE; NAR v9 · 30 s · 32 steps | CUDA 13 | 29.999 | 31.134 | 5.491 | 7.32 / +5.79 | 3.51 |
| YuE2 · yue2.cpp 892b9b0 · 3B Q8_0 + F32 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 13 | 3.999 | 24.461 | 0.158 | 6.08 / +4.55 | 3.50 |
| ACE-Step 1.5 · acestep.cpp 6e0e414 · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | CUDA 13 | 30.000 | 20.569 | 0.318 | 9.37 / +7.85 | 2.20 |
| MiniMax Music3 · minimaxmusic.cpp b79d37f · Minimal Q3_K_M / Q4_K_M + F32 vocoder · 3.2 s · 2 steps | CUDA 13 | 3.193 | 35.096 | 1.091 | 8.82 / +7.30 | 4.71 |
| MiniMax Music3 · minimaxmusic.cpp b79d37f · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | CUDA 13 | 3.193 | 65.319 | 1.697 | 14.67 / +13.13 | 8.95 |
| YuE2 · yue2.cpp 892b9b0 · 3B Q8_0 + F32 VAE; NAR v9 · 30 s · 32 steps | CUDA 12 | 29.999 | 37.919 | 5.538 | 7.31 / +5.78 | 3.49 |
| YuE2 · yue2.cpp 892b9b0 · 3B Q8_0 + F32 VAE; NAR v9 · 100 tokens · 2 steps | CUDA 12 | 3.999 | 24.667 | 0.158 | 6.08 / +4.54 | 3.47 |
| ACE-Step 1.5 · acestep.cpp 6e0e414 · Turbo Q8_0 + BF16 embedder/VAE · 30 s · 8 Euler steps | CUDA 12 | 30.000 | 20.825 | 0.318 | 9.37 / +7.84 | 2.16 |
| MiniMax Music3 · minimaxmusic.cpp b79d37f · Minimal Q3_K_M / Q4_K_M + F32 vocoder · 3.2 s · 2 steps | CUDA 12 | 3.193 | 37.674 | 1.081 | 8.83 / +7.30 | 4.71 |
| MiniMax Music3 · minimaxmusic.cpp b79d37f · Quality Q8_0 + F32 vocoder · 3.2 s · 2 steps | CUDA 12 | 3.193 | 86.020 | 1.745 | 14.67 / +13.14 | 8.91 |
<!-- music-gpu-wsl:end -->

<!-- radeon-gpu-pair:start -->
#### Stable Audio 3 Small Q8_0 · Radeon iGPU

Windows, AMD Radeon Graphics (gfx1036, UMA). The same `audio.cpp 5e7273db` revision generated 4 seconds in four steps with seed 42 on each backend. Cold is the first run. Warm is the median of three requests. WDDM dedicated and shared columns show process-memory peaks sampled at one-second intervals.

| Backend | Cold (s) | Warm (s) | WDDM dedicated peak (GiB) | WDDM shared peak (GiB) |
| --- | ---: | ---: | ---: | ---: |
| HIP | 4.609 | 2.953 | 1.39 | 0.04 |
| Vulkan | 6.281 | 3.454 | 1.17 | 0.38 |

[Individual runs and WAV hashes](../../docs/music-gpu-benchmarks.json)
<!-- radeon-gpu-pair:end -->

<!-- native-radeon-pair:start -->
#### YuE2-3B Q8_0 · Radeon iGPU

Same 12-second saved song and Radeon iGPU, four steps. Cold includes process startup. Warm is the median of three requests with weights resident. WDDM peaks are sampled per process on this adapter.

| Backend | Cold (s) | Warm (s) | WDDM dedicated peak (GiB) | WDDM shared peak (GiB) |
| --- | ---: | ---: | ---: | ---: |
| HIP 188d13b3 | 22.749 | 18.483 | 0.28 | 4.39 |
| Vulkan 892b9b03 | 30.923 | 24.438 | 1.82 | 2.59 |

[Individual runs and WAV hashes](../../docs/music-gpu-benchmarks.json)
<!-- native-radeon-pair:end -->

**RVC voice conversion on GPU (5.0 development builds)**

Windows on one RTX 4090 24 GB, using the same voice checkpoint and FAISS index with a retrieval blend of 0.5. Each run converts an existing vocal to a new voice.

| RVC backend | Input | First run, empty cache | Restart, saved cache | Warm request | Peak GPU memory |
| --- | ---: | ---: | ---: | ---: | ---: |
| vocal-fast-rs · TensorRT-RTX | 4 s | 21.046 s | 1.694 s | 64.83 ms | 3.91 GiB |
| audio.cpp · CUDA | 4 s | 4.639 s | 4.448 s | 204.84 ms | 4.09 GiB |
| vocal-fast-rs · TensorRT-RTX | 30 s | 24.258 s | 2.009 s | 279.29 ms | 5.51 GiB |
| audio.cpp · CUDA | 30 s | 5.334 s | 5.477 s | 942.17 ms | 6.83 GiB |

Startup runs include process launch through the first saved WAV. Columns show medians of two empty-cache launches, three cached launches and three warm requests. Both warm paths use the same local HTTP client, from POST through writing the WAV. Peak memory is the median whole-card peak across three cached processes during warm conversion, including desktop usage. The engines search different numbers of FAISS lists; timing alone does not measure voice quality. [Individual runs and build identifiers](../../docs/rvc-gpu-benchmarks.json)

### Speed reported by installations

How long a 3-minute song takes, by graphics card. These are averages from what YuE2 Studio installations reported
over 30 days (version 3.8, loading the models included; people run different model sets, so yours can differ).

| Your graphics card | A 3-minute song | Speed |
|---|---|---|
| NVIDIA, 24 GB or more | about 1 minute | 2.7× faster than real time |
| NVIDIA, 16 GB | about 2 minutes | 1.6× |
| NVIDIA, 12 GB | about 2 minutes | 1.4× |
| NVIDIA, 8 GB or less | about 5 minutes | 0.6× |
| AMD, 16 GB or more (Vulkan) | 3–4 minutes | 0.9× |
| Intel Arc, 12 GB (Vulkan) | about 7 minutes | 0.4× |
| Apple silicon, 24 GB or more | about 8 minutes | 0.4× |
| Apple silicon, 16 GB | about 15 minutes | 0.2× |
| AMD, 8 GB or less (Vulkan) | about an hour | 0.05× |
| No graphics card (processor) | about 2.5 hours | 0.02× |

On our machine (RTX 4090, Ryzen 9 9950X3D, Windows 11, the Q8_0 set): a 1-minute song in 20 s, a 4-minute song in
54 s, a 10-minute song in about 3 minutes; the card holds 5–8 GB while it works.

Next: [get started](getting-started.md) · [choose models and engines](models-and-runtimes.md) ·
[create and edit music](creating-music.md) · [get help](community.md#report-a-problem).
