# Models and runtimes

[Back to YuE2 Studio](../../README.md)

## What runs where

| Part | NVIDIA | AMD, Intel | No graphics card |
| --- | --- | --- | --- |
| Making songs · yue2.cpp | CUDA | Vulkan | processor |
| Training a LoRA · music-train | CUDA | not available | not available |
| Style by ear · MOSS-Music | CUDA | processor | processor |
| Tempo · Beat This! | processor | processor | processor |
| Stems · HT-Demucs | CUDA | Vulkan | processor |
| Stems · Mel-Band RoFormer | CUDA | Vulkan | processor |
| Restore after compression · Apollo | CUDA | Vulkan | processor |
| Karaoke timing · Parakeet | CUDA | Vulkan | processor |
| Karaoke timing · Whisper | CUDA | processor | processor |
| Writing assistant · llama.cpp | CUDA | Vulkan | processor |
| Audio to MIDI · MuScriptor | CUDA | processor | processor |

**The paths for AMD and Intel cards (Vulkan) and work on the processor are
experimental:** they exist for computers where the studio would not run at all otherwise.
The main, tested path is an NVIDIA card.

- Training needs an NVIDIA card with 11 GB of video memory or more. The trainer has no other
  path, and on the processor one run would take days, so on any other machine the studio
  does not offer the training files.
- On AMD and Intel cards HT-Demucs separates the stems through Vulkan in the audio.cpp server and
  Mel-Band RoFormer through its Vulkan build; MuScriptor (audio to MIDI) runs on the processor there.
- Karaoke's Parakeet runs on transcribe.cpp, the engine Dub Studio and Speech Studio use: its
  CUDA 13 or CUDA 12 build by the card and driver (on Linux when the system has that CUDA's
  cuBLAS), its Vulkan build on any other card, its processor build without one; on a Mac, Metal
  on Apple silicon and the processor on Intel.
- The tempo model runs on the processor on every machine, in 5 to 7 seconds a song.
- "Auto" in a device choice takes the card when its runtime is installed, and the processor
  otherwise. The same table is in the studio, under Settings → Models.

## Models

A runnable YuE2 installation is a **backbone** (the 3B model that writes the score and the
audio codes) and the **VAE** (turns them into 48 kHz stereo). **SheetSage2** is optional:
without it the studio generates, but cannot transcribe recordings for covers.

Every set also carries the **decoder companion**, 134 MB: Mothersuperior's decoder adapter,
published as the pair of the tokenizer head the LoRA trainer turns songs into codes with. The
trainer keeps it frozen under every LoRA, so a LoRA is trained on the decoder it is heard
through. A render merges it on **Auto** (Advanced - output) only under a LoRA trained in the
studio; otherwise the song is decoded by the YuE2 checkpoint alone, as its authors, audio.cpp and
ComfyUI render it. On and Off force it either way. In HOT-Step's round trip of six real songs the
decoder without it narrowed the stereo (left/right correlation 0.78–0.92 against the
originals' ~0.65), with it 0.57–0.72.

| Your GPU | Set | Download |
| --- | --- | --- |
| 12 GB VRAM and above | Full native — BF16 backbone, original weights | 9.9 GB |
| 8 GB and above | Quality — Q8_0 backbone, near lossless | 5.1 GB |
| 7 GB and above | Balanced — Q6_K backbone | 4.3 GB |
| 5.5 GB and above | Light — Q5_K_M backbone | 4.0 GB |

Sizes include SheetSage2 with the MERT-v2 encoder it reads (Q8_0, F32 in the native set) and the
decoder companion. The studio
detects your card and preselects the set, but the download is always your decision; the model
manager also builds a custom mix role by role. Q5_K_M is the lightest quantisation published
for YuE2.

The GGUF files come from [Serveurperso/YuE2-GGUF](https://huggingface.co/Serveurperso/YuE2-GGUF),
pinned to revision `e630f2b`, the decoder companion from
[Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4](https://huggingface.co/Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4),
pinned to revision `e2e63d8`; all are checked by size and SHA-256. They are written to, and
can be dropped into by hand at:

- `<the studio's folder>\data\models\yue2-cpp\` — the portable folder, or the folder
  you installed into
- `%LOCALAPPDATA%\YuE2 Studio\models\yue2-cpp\` — only for an installation into a
  read-only location

A file placed by hand with the exact catalogue name is recognised and never downloaded again.

**Your own YuE2 model.** A YuE2-3B fine-tune in GGUF made for yue2.cpp goes into
`models\yue2-cpp\own\` and is chosen under Settings - Models - Your own YuE2 model; the engine then
runs it in place of the set's backbone, with the set's VAE and transcriber. The studio does not
download or check a file you put there, and its licence is its author's. The same list has
Community models: the merges of Ruach Studio's
[YuE2-RS-Finetunes](https://huggingface.co/goldhub/YuE2-RS-Finetunes) in Q8_0 (3.8 GB) and Q6_K
(2.9 GB), downloaded into that folder when chosen, pinned to revision `b371be9` and checked by
SHA-256; their weights are under CC BY-NC-SA 4.0 (non-commercial, share-alike, credit the authors).
Beside them are the six merges of
[YuE2-3B-Ruach_Studio-IRA-Merge](https://huggingface.co/goldhub/YuE2-3B-Ruach_Studio-IRA-Merge)
(revision `17356b2`, CC BY-NC 4.0): IRA, with Mothersuperior's Instrumental and Real Audio adapters
in the weights; IRAE, IRAE v2 and v3, which add monsterovich's Epic Trailer at three strengths; and
the experimental IRAE-IR and IRAE-IR v2 with his Industrial Rock, for rock instrumentals. The
adapters inside a merge are not loaded again on top of it.
With an own or community model the decoder companion stays off on Auto, and LoRA training and
dataset preparation still run on the set's backbone.

<details>
<summary><b>Every file, with direct download links</b></summary>

| File | Role | Size |
| --- | --- | --- |
| [`YuE2-3B-BF16.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/YuE2-3B-BF16.gguf) | backbone | 6.67 GB |
| [`YuE2-3B-Q8_0.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/YuE2-3B-Q8_0.gguf) | backbone | 3.55 GB |
| [`YuE2-3B-Q6_K.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/YuE2-3B-Q6_K.gguf) | backbone | 2.74 GB |
| [`YuE2-3B-Q5_K_M.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/YuE2-3B-Q5_K_M.gguf) | backbone | 2.44 GB |
| [`YuE2-Vae-F32.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/YuE2-Vae-F32.gguf) | VAE, every set | 506 MB |
| [`SheetSage2-F32.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/SheetSage2-F32.gguf) | transcriber | 218 MB |
| [`SheetSage2-Q8_0.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/SheetSage2-Q8_0.gguf) | transcriber | 101 MB |
| [`MERT-v2-FullSong-F32.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/MERT-v2-FullSong-F32.gguf) | the transcriber's encoder, beside SheetSage2-F32 | 2.36 GB |
| [`MERT-v2-FullSong-Q8_0.gguf`](https://huggingface.co/Serveurperso/YuE2-GGUF/resolve/e630f2b8f6aedf8254c3186438633c1b5eeaf80a/MERT-v2-FullSong-Q8_0.gguf) | the transcriber's encoder, beside SheetSage2-Q8_0 | 860 MB |
| [`nar_lora_joint_v9.safetensors`](https://huggingface.co/Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4/resolve/e2e63d859f3af879baf1b4d4e9f22d1eeda6fde5/nar_lora_joint_v9.safetensors) | decoder companion, every set | 134 MB |

</details>

### Everything else the studio downloads

Each part is downloaded when you first use it, and each file can be downloaded by hand
and put into the studio's `data` folder — `<the studio's folder>\data\`, or
`%LOCALAPPDATA%\YuE2 Studio\` for an installation into a read-only location. A file with
the exact name in the listed folder is recognised and not downloaded again.

<details>
<summary><b>Training, listening, lyrics, stems and the assistant — direct links</b></summary>

**Training a LoRA** — [scragnog/YuE2-GGUF](https://huggingface.co/scragnog/YuE2-GGUF), pinned to `eb7de09`

| File | What for | Size | Put in `data\` |
| --- | --- | --- | --- |
| [`yue2_3b_int8_convrot.safetensors`](https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/checkpoints/yue2_3b_int8_convrot.safetensors) | the model the LoRA is trained on | 3.69 GB | `training\models\` |
| [`yue2-vae-standard-f32.gguf`](https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/yue2-vae-standard-f32.gguf) | audio to latents | 506 MB | `training\models\` |
| [`yue2-tok-f16.gguf`](https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/yue2-tok-f16.gguf) | audio to codes | 1.13 GB | `training\models\` |
| [`sheetsage2-f16.gguf`](https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/sheetsage2-f16.gguf) | the songs' scores | 1.27 GB | `training\models\` |
| [`mms-fa-f32.gguf`](https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/mms-fa/mms-fa-f32.gguf) | lyric timing | 1.18 GB | `training\models\` |

**Describing songs by ear** (optional)

| File | What for | Size | Put in `data\` |
| --- | --- | --- | --- |
| [`moss-aud-f16.gguf`](https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/main/moss-aud-f16.gguf) | MOSS-Music-8B, the ear | 1.61 GB | `training\models\moss\` |
| [`moss-lm-q8_0.gguf`](https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/main/moss-lm-q8_0.gguf) | MOSS-Music-8B, the words | 8.11 GB | `training\models\moss\` |
| [`beat_this.onnx`](https://github.com/mosynthkey/beat_this_cpp/raw/main/onnx/beat_this.onnx) | Beat This!, the tempo | 79 MB | `training\models\audio-facts\` |

**Lyrics by ear** — only for songs no lyric database knows; pick one recogniser

| Files | What for | Size | Put in `data\` |
| --- | --- | --- | --- |
| [faster-whisper-large-v3](https://huggingface.co/Systran/faster-whisper-large-v3/tree/main): `config.json`, `model.bin`, `preprocessor_config.json`, `tokenizer.json`, `vocabulary.json` | Whisper large-v3 | 2.9 GB | `karaoke\models\whisper\faster-whisper-large-v3\` |
| [`parakeet-tdt-0.6b-v3-Q8_0.gguf`](https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/d8d8c32cb2017c57e5307767343e97870f8a7f53/parakeet-tdt-0.6b-v3-Q8_0.gguf) | Parakeet TDT v3, European languages | 0.7 GB | `karaoke\models\asr-gguf-parakeet-v3\` |
| [`parakeet-ultra-Q8_0.gguf`](https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/a1539be1b849ea7463b88343e5277b373d0c4ea9/parakeet-ultra-Q8_0.gguf) | Parakeet Ultra, fewer wrong words in Russian and in noise | 0.7 GB | `karaoke\models\asr-gguf-parakeet-ultra\` |
| [`transcribe-windows-cuda13-x64.zip`](https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-cuda13-x64.zip), [`transcribe-windows-vulkan-x64.zip`](https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-vulkan-x64.zip), [`transcribe-windows-cpu-x64.zip`](https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-cpu-x64.zip) | transcribe.cpp, the engine Parakeet runs on: one build, the one for your card, unpacked | 5-131 MB | `karaoke\runtime\transcribe-engine\` (CUDA), `transcribe-engine-vulkan\`, `transcribe-engine-cpu\` |

**The audio.cpp server** — HT-Demucs and MuScriptor run in it; one build, the one for your card, unpacked

| File | What for | Size | Put in `data\` |
| --- | --- | --- | --- |
| [`audiocpp-server-win-cuda13-x64.zip`](https://github.com/timoncool/audio.cpp/releases/download/server-31cb1a4a/audiocpp-server-win-cuda13-x64.zip), [`audiocpp-server-win-cuda12-x64.zip`](https://github.com/timoncool/audio.cpp/releases/download/server-31cb1a4a/audiocpp-server-win-cuda12-x64.zip), [`audiocpp-server-win-vulkan-x64.zip`](https://github.com/timoncool/audio.cpp/releases/download/server-31cb1a4a/audiocpp-server-win-vulkan-x64.zip), [`audiocpp-server-win-cpu-x64.zip`](https://github.com/timoncool/audio.cpp/releases/download/server-31cb1a4a/audiocpp-server-win-cpu-x64.zip) | audio.cpp's server: CUDA 13, CUDA 12, Vulkan or the processor | 20-185 MB | `audiocpp\runtime\audiocpp\` (CUDA 13), `audiocpp-cuda12\`, `audiocpp-vulkan\`, `audiocpp-cpu\` |
| [`libcufft-windows-x86_64-12.4.0.43-archive.zip`](https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip) | `cufft64_12.dll` out of it, for the CUDA 13 build | 160 MB | `audiocpp\runtime\audiocpp\` |

**Stems and vocals for recognition**

| File | What for | Size | Put in `data\` |
| --- | --- | --- | --- |
| [`htdemucs-6stems-f16.gguf`](https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/462b4cec041e06ffb3d8fcf0d8c655051b1cd9f5/HTDemucs-6stems-GGUF/htdemucs-6stems-f16.gguf) | HT-Demucs, six stems, audio.cpp's own GGUF | 55 MB | `separation\models\htdemucs\` |

**MIDI from audio** — one of the models, in the audio.cpp server above
| [muscriptor-small](https://huggingface.co/cocktailpeanut/muscriptor-small/tree/31a8f75d6a8b5383fd71ad1371dc1620389ab722): `config.json`, `model.safetensors` | fast, 103M | 0.4 GB | `midi\models\muscriptor-small\` |
| [muscriptor-medium](https://huggingface.co/cocktailpeanut/muscriptor-medium/tree/27246ba68bd4d8f98bdec10a6edf8d7cf42a8826): `config.json`, `model.safetensors` | balanced, 307M | 1.2 GB | `midi\models\muscriptor-medium\` |
| [muscriptor-large](https://huggingface.co/cocktailpeanut/muscriptor-large/tree/87f4bf981f56f90fb5043153b3f54af3c3053da9): `config.json`, `model.safetensors` | best, 1.4B | 5.5 GB | `midi\models\muscriptor-large\` |

MuScriptor by Kyutai & Mirelo, weights CC BY-NC 4.0 (non-commercial); the mirror carries the
official files byte for byte, without the Hugging Face sign-in.

**The writing assistant** — one of

| File | Size | Put in `data\` |
| --- | --- | --- |
| [`gemma-4-E4B_q4_0-it.gguf`](https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/main/gemma-4-E4B_q4_0-it.gguf) | 4.80 GB | `assistant\models\` |
| [`gemma-4-12b-it-qat-q4_0.gguf`](https://huggingface.co/google/gemma-4-12b-it-qat-q4_0-gguf/resolve/main/gemma-4-12b-it-qat-q4_0.gguf) | 6.50 GB | `assistant\models\` |

</details>

## The engine and the DLLs it needs

```text
yue-server.exe → ggml.dll, ggml-base.dll            shipped inside the app
  loads at run time, whichever the machine can use:
  ├─ ggml-cuda.dll     → cublas64_13.dll, cublasLt64_13.dll (downloaded once), nvcuda.dll (NVIDIA driver)
  ├─ ggml-vulkan.dll   → vulkan-1.dll (every AMD, Intel and NVIDIA driver)
  └─ ggml-cpu-*.dll    nine builds, from SSE4.2 to AVX-512; the best one for the processor is picked
  + msvcp140, vcruntime140, vcruntime140_1, vcomp140   Visual C++ runtime, shipped app-local
```

**Shipped inside the app.** `yue-server.exe`, every `ggml*.dll`, built from the pinned
yue2.cpp commit with all backends, and the Visual C++ runtime it needs live in
`resources\yue2-cpp\` beside the main executable. Nothing is installed into Windows. Settings → Local engine chooses the compute device: Auto (CUDA on NVIDIA,
Vulkan on AMD and Intel, the processor without a GPU), or CUDA, Vulkan or the processor
explicitly. On Vulkan the studio turns on the engine's FP16 clamp: without it AMD Radeon
integrated graphics rendered pure silence and the engine crashed encoding it.

**Downloaded once, on the first engine start — on NVIDIA only.**

| File(s) | Where from | Size | Why |
| --- | --- | --- | --- |
| `cublas64_13.dll`, `cublasLt64_13.dll` | NVIDIA's redistributable [`libcublas-windows-x86_64-13.5.1.27-archive.zip`](https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip) | 391 MB (zip) | The CUDA linear algebra `ggml-cuda.dll` is linked against; too large, and under NVIDIA's licence, to bundle. |

A machine with the CUDA 13 toolkit already has cuBLAS on its `PATH` and downloads nothing.
Behind a proxy, take the two DLLs from the archive's `bin\` folder and drop them next to
`yue-server.exe`; the studio finds and uses them.

Next: [get started](getting-started.md) · [create and edit music](creating-music.md) ·
[compare performance](benchmarks.md#how-fast-is-it) · [get help](community.md#report-a-problem).
