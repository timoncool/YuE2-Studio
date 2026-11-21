# Native RVC Burn runtime

The Training panel can train an RVC voice from isolated voice recordings on an NVIDIA GPU. The Studio installs the resulting Safetensors checkpoint into the voice catalogue. Choose the voice on a song with separated stems to convert its vocal and save a new mixed take.

In the Studio on Windows x64, open **Training**, choose **RVC · Burn CUDA**, and select **Install native RVC**. The Studio chooses a CUDA package for the detected NVIDIA card and driver. The installer verifies the pinned SHA-256 of the runtime, dependency archives and weights. It saves the components in the Studio data folder and displays progress and cancellation. In MCP, use `rvc_training_install_runtime` and `rvc_training_status`.

A manually prepared runtime can still be placed in `rvc/burn` in the Studio data folder. To use a shared manual installation, set `YUE_RVC_BURN_ROOT` to its absolute path before starting the Studio.

Build the CUDA CLI from [kingwingfly/voice at 0639605](https://github.com/kingwingfly/voice/tree/06396059536f5894d5872a86bf00fe30d6ff62f9) with `cargo build --locked -p rvc-cli --no-default-features --features cuda`. The Windows build and runtime were verified with CUDA Toolkit 13.3, the upstream FFmpeg shared dependencies, and the CPU ONNX Runtime 1.28.2. Put `rvc.exe`, `onnxruntime.dll`, and the required FFmpeg and CUDA DLLs in the runtime folder or supply the dependency DLLs on the process PATH. Keep all executable and dependency files from a trusted build. The Windows runtime uses a native Rust CLI, Burn CUDA and ONNX, with no Python dependencies.

Put these exact feature models in the same folder. The training warm-start checkpoints are only needed for training.

| File | Pinned source | SHA-256 |
| --- | --- | --- |
| `vec-768-layer-12.onnx` | [MoeSS-SUBModel](https://huggingface.co/NaruseMioShirakana/MoeSS-SUBModel/resolve/3ff2e37e161ebcb3c4711e7ec870472342260842/vec-768-layer-12.onnx) | `b3886e7dff1495cda514f94f4680a7b1261e05d6929f5c764cdb17934b413c2a` |
| `rmvpe.onnx` | [VoiceConversionWebUI](https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/e6d0c1a17da07c33557852f9dfa2bd44cc75737d/rmvpe.onnx) | `5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd` |
| `f0G48k.pth` | [VoiceConversionWebUI](https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/e6d0c1a17da07c33557852f9dfa2bd44cc75737d/pretrained_v2/f0G48k.pth) | `b5d51f589cc3632d4eae36a315b4179397695042edc01d15312e1bddc2b764a4` |
| `f0D48k.pth` | [VoiceConversionWebUI](https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/e6d0c1a17da07c33557852f9dfa2bd44cc75737d/pretrained_v2/f0D48k.pth) | `2269b73c7a4cf34da09aea99274dabf99b2ddb8a42cbfb065fb3c0aa9a2fc748` |

Add at least ten seconds of isolated voice audio in Training, open the Train step, and choose **RVC · Burn CUDA**. Select **Install voice** on a finished run. Use the installed voice with the normal vocal conversion workflow. Existing `.pth` voices and their optional `.index` files stay available in the same catalogue.
