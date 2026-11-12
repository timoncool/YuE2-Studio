//! The SHA-256 of every pinned download, as its publisher states it: GitHub's release asset digest, the LFS object
//! id on Hugging Face (and, for a small file kept in git there or on GitHub, the hash of the file whose git object id
//! is the pinned revision's), NVIDIA's redistributable manifests, PyPI's file hashes. A downloaded file whose hash
//! differs is removed and the download fails; a neighbour's file is taken only with the pin's hash.

use std::path::Path;

use anyhow::{bail, Result};

const PINNED: &[(&str, &str)] = &[
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg", "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9"),
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg", "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/linux-x86_64/cuda_cudart-linux-x86_64-12.9.79-archive.tar.xz", "1f6ad42d4f530b24bfa35894ccf6b7209d2354f59101fd62ec4a6192a184ce99"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/windows-x86_64/cuda_cudart-windows-x86_64-12.9.79-archive.zip", "179e9c43b0735ffe67207b3da556eb5a0c50f3047961882b7657d3b822d34ef8"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/linux-x86_64/libcublas-linux-x86_64-11.11.3.6-archive.tar.xz", "045e6455c9f8789b1c7ced19957c7904d23c221f4d1d75bb574a2c856aebae98"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-11.11.3.6-archive.zip", "67b0934a6359e4ee26fff823c356021589d392c4fd49ca12624f570edc08e2b9"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.1.4-archive.zip", "d534d98b0b453a98914dbf3adf47d7e84b55037abf02f87466439e1dcef581ed"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip", "c946e1c825e05895747a95ed4fee18030b08052c09783b9b7b19818fd2e31f58"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-11.4.1.4-archive.tar.xz", "b0e65af59b0c2f6c8ed9f5552a9b375890855b7926ae2c0404d15dcf2565bda4"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-11.4.1.4-archive.zip", "f26f80bb9abff3269c548e1559e8c2b4ba58ccb8acc6095bbc6404fc962d4b80"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip", "69d0ad8dc3a1be66f01a748a8206d0ceaafa56939474663fbe790dc3e91d2009"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcurand/linux-x86_64/libcurand-linux-x86_64-10.3.10.19-archive.tar.xz", "48281b4caadb1cf790d44ac76b23c77d06f474c0b1799814f314aafec9258ad6"),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-9.25.0.15_cuda12-archive.tar.xz", "0fe244321b401758cbb8430d1997f494add139405a4a4091338bcbb6f5024fa8"),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-9.25.0.15_cuda12-archive.zip", "06e94f70c52d7335b7ed8044eed28ce963b7fd59d8c2c446ffc60e695fccad91"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "e4f53a8ca8c5d6e8c492d0d0a3d565ecb59a751b19cfdaa4f6da0ab2104c1702"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cuda13-x64.tar.gz", "807f59113f17eb706bc7289155bdf507d05ae01650a5c2ccd9685f5574e9cd04"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cpu-x64.tar.gz", "65eb3cf26dfed0a23f01906a9f179ae7ab503f2bdac4f411e49ec902a5addafd"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-macos-arm64.tar.xz", "d0fc45181d31dcceea99c3828378f4a7376e52e7fdbc43e29a07e806785b4c9a"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-cuda13-x64.zip", "c89128404d29c52ac0efed1a1072a76b277ccf13a33247611032746932e8d032"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-cpu-x64.zip", "0b212654ea598503992119328daec051bbce5f14b73808c5cc588f278817f908"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-12.4-x64.zip", "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-13.4-x64.zip", "738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/cudart-llama-b12248-bin-ubuntu-cuda-12.8-x64.tar.gz", "ad244917cc8701d0fa517d6500dafd55bb74e29f26fb3a051ef31c8eed1cb90d"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/cudart-llama-b12248-bin-ubuntu-cuda-13.5-x64.tar.gz", "4b4dab68e9bf809b96fc77c900d22aea5639ade7eedf4b558d5382d2191bdcf8"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-macos-arm64.tar.gz", "7f643b4d35803ccc69270302f3368d9e8a40f56f74ef5a266e6f686d9dc7d3c6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-cuda-12.8-x64.tar.gz", "508214544c8e36c2a2a614254231524a10a0857a8a025f7e4be1f437defae0ef"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-cuda-13.5-x64.tar.gz", "ac18e01410e0114e1d5f2f0f3bdcd9a98d83000b5179f543342c8c51c33143b7"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-vulkan-x64.tar.gz", "092d7097cf90c1d1a22d0bba5147500f2420bdb9630db8734230f35bcb664bed"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-x64.tar.gz", "6b3a530b44f6b0a4ae4872d3b3c8d1edc2ac6bba7d8bd211329a4ba644c0dc65"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cpu-x64.zip", "bb306bfd390f8782c0acaba490505d12f07392791b48f1dd039387a5bacf1181"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cuda-12.4-x64.zip", "3f8a8f15b50f2f1e34ad2975d8679a009ec83a83e6f440322cb22f28d8068d60"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cuda-13.5-x64.zip", "ae0a1e064a94db82f3352c843ffa96eb190254fefafc29ce0213798c549a2799"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-vulkan-x64.zip", "3745e446c88e18cf20fd52f9df62867beb60d12fd0208c130fb4663117425826"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz", "c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-gpu_cuda12-1.31.1.tgz", "57cd51f677eca19a269cdba31b0c16c15d73a8feea41f170aa8b750447ab1f70"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz", "2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip", "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-gpu_cuda12-1.31.1.zip", "5cbf65edc93dc7bb3296249b1b6e1b2228e575902af64d6163d0da2103cef9d5"),
    ("https://github.com/mosynthkey/beat_this_cpp/raw/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/onnx/beat_this.onnx", "c5c1466e08abdb03fdeb50668a06f244b787d564c212490482231a9cfbe9ccbd"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-2ef93a1c/ace-caption-windows-x64.zip", "a2d7495bee6f01c3b6c55c79aba23d91169d9a3ce70a4d0ed9a24bf23c1865b4"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-2ef93a1c/hotstep-tools-cuda-windows-x64.zip", "07e157864bec7bba62f491a24c54881810d6943cca4af986f14cc41a8e972390"),
    ("https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/config.json", "56a6d8110d311f19c8f0471e562832c7527f146b567275bfca59fcf7c184da9a"),
    ("https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/model.bin", "d01c3014881c9c6f3133c182f3d2887eb6ca1c789a7538c5c007196857a0a6a9"),
    ("https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/tokenizer.json", "fb7b63191e9bb045082c79fd742a3106a12c99513ab30df4a0d47fa6cb6fd0ab"),
    ("https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/vocabulary.txt", "34ce3fe1c5041027b3f8d42912270993f986dbc4bb34cf27f951e34a1e453913"),
    ("https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/config.json", "a9306624f5ec14270a014b647e5c316b6e03a662c369758d1b90697a7b0655b9"),
    ("https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/model.bin", "69f74147e3334731bc3a76048724833325d2ec74642fb52620eda87352e3d4f1"),
    ("https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/preprocessor_config.json", "7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711"),
    ("https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/tokenizer.json", "6d8cbd7cd0d8d5815e478dac67b85a26bbe77c1f5e0c6d76d1ce2abc0e5f21ca"),
    ("https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/vocabulary.json", "c69260f2ab26d659b7c398f9a2b2b48ed0df16c3b47d7326782fd9cba71690c1"),
    ("https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/config.json", "3622a2ddc41ec0e0fd4e68c13c6830f03b90c38d89aaad184de02c8c642cf807"),
    ("https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/model.bin", "9b45e1009dcc4ab601eff815b61d80e60ce3fd8c74c1a14f4a282258286b51ae"),
    ("https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/tokenizer.json", "fb7b63191e9bb045082c79fd742a3106a12c99513ab30df4a0d47fa6cb6fd0ab"),
    ("https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/vocabulary.txt", "34ce3fe1c5041027b3f8d42912270993f986dbc4bb34cf27f951e34a1e453913"),
    ("https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/config.json", "b55496ac7940a7ae47d2c01eab40edfd8701feec1229d9cce3b40014383fb828"),
    ("https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/model.bin", "3e305921506d8872816023e4c273e75d2419fb89b24da97b4fe7bce14170d671"),
    ("https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/tokenizer.json", "fb7b63191e9bb045082c79fd742a3106a12c99513ab30df4a0d47fa6cb6fd0ab"),
    ("https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/vocabulary.txt", "34ce3fe1c5041027b3f8d42912270993f986dbc4bb34cf27f951e34a1e453913"),
    ("https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/config.json", "a73a28cdfe1c43ccc7202fa333d1f89c202477271407ae9a7f19afa52039cac8"),
    ("https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/model.bin", "dcb76c6586fc06cbdac6dd21f14cfd129cc4cdd9dce19bf4ffa62e59cbe6e6d1"),
    ("https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/tokenizer.json", "fb7b63191e9bb045082c79fd742a3106a12c99513ab30df4a0d47fa6cb6fd0ab"),
    ("https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/vocabulary.txt", "34ce3fe1c5041027b3f8d42912270993f986dbc4bb34cf27f951e34a1e453913"),
    ("https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/462b4cec041e06ffb3d8fcf0d8c655051b1cd9f5/HTDemucs-6stems-GGUF/htdemucs-6stems-f16.gguf", "1db9bd58af6995d8caab209c99f5a284212e5bd0654fbfd7e50ce1d5ac140ba9"),
    ("https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q8_0.gguf", "2cd84c9f24513749b0cb1a6ab3e3be5c5e2f7d0e8533e50512c1f394d2828a73"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-large/resolve/87f4bf981f56f90fb5043153b3f54af3c3053da9/config.json", "16bedd02b18770e43740419b0d5777f231047e96e8987f498e8a1123c39c9852"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-large/resolve/87f4bf981f56f90fb5043153b3f54af3c3053da9/model.safetensors", "ac4eb6ea87dfc26b6ca6b954c6b967ab87ad4c7d08e078b25214f13ed051f397"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-medium/resolve/27246ba68bd4d8f98bdec10a6edf8d7cf42a8826/config.json", "43e13a70fc9ae0af36b7447c06f3eac2282daeb69d79c1ff840ede7fdaa26a3b"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-medium/resolve/27246ba68bd4d8f98bdec10a6edf8d7cf42a8826/model.safetensors", "ac80adbdf85d87231735fd948af7013441c0afced316c4e9067fd5d8a7fb97ec"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-small/resolve/31a8f75d6a8b5383fd71ad1371dc1620389ab722/config.json", "3008fc481e4a1cd978e337eb3759260c270892204db5039235ac939e1f42aeb2"),
    ("https://huggingface.co/cocktailpeanut/muscriptor-small/resolve/31a8f75d6a8b5383fd71ad1371dc1620389ab722/model.safetensors", "bbd482c786b895cf7d8f44185073d951adae2ebb8a66f82ca84cd1f84569549c"),
    ("https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/config.json", "b0253ea6c0d3bea6b1e19e91a02acfd3b53f4467362efcb5a3e6b16c9b3a9b7e"),
    ("https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/model.bin", "e76620f83d5f5b69efd3d87e3dc180c1bd21df9fbebacfd4335e5e1efcc018da"),
    ("https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/preprocessor_config.json", "7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711"),
    ("https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/tokenizer.json", "297b13372ac43916285644fb9687add3cc62ee2a1adb60da3dc25cc94c1871fd"),
    ("https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/vocabulary.json", "c69260f2ab26d659b7c398f9a2b2b48ed0df16c3b47d7326782fd9cba71690c1"),
    ("https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf/resolve/29d097773436b69ff9feafd636ab4cf873786537/gemma-4-12b-it-qat-q4_0.gguf", "93567e57a8fe10b23569b9d9ec38cd005deedf71e29477c421a4b83f418a538b"),
    ("https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/4b4a2c1d584be7264f87aac328a1bc739ce81b6c/gemma-4-E4B_q4_0-it.gguf", "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee"),
    ("https://huggingface.co/nerualdreming/MOSS-Music-8B-Instruct-GGUF/resolve/0b4eac01086c26baf93238aa8b142ec07b3ae56d/moss-lm-q4_k_m.gguf", "bc2783b68fee6dac606599c4b8f15bff5e37913b8d10fbfb8c550f5667dee3e8"),
    ("https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/9fe86435bd542bf29cbf4984e9e0ab0ea8ca7e00/moss-aud-f16.gguf", "014b971719a9b46586ea78ea3d15f39ac0f1932678e30c72fa63222f8aea5f79"),
    ("https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/9fe86435bd542bf29cbf4984e9e0ab0ea8ca7e00/moss-lm-q8_0.gguf", "482e1e82c66f819401708c82b3eaa1dd6635c68a211f03b645b0055d0b7b031b"),
    ("https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/checkpoints/yue2_3b_int8_convrot.safetensors", "96fe199377309001ed8cd26a944baeee8cc31a20ba7c36d1d3c0a7e1f4149db6"),
    ("https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/mms-fa/mms-fa-f32.gguf", "2a7df904c5f7d18f595da03e9636541686dfe41292d983e4dc7bfeb11b71b7c3"),
    ("https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/sheetsage2-f16.gguf", "6a205c7ca90f60b388d925c0e4aea8f4ed8bb4404520bb49dbd095d1b7320bd1"),
    ("https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/yue2-tok-f16.gguf", "aaea8c9fb75bf83baf3b99bac27a0eba61029e747df703a11e91a53309695687"),
    ("https://huggingface.co/scragnog/YuE2-GGUF/resolve/eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7/yue2-vae-standard-f32.gguf", "c4ffc363b246f404c1d150075da4c41c4a031524036042ce45aeb500d4d5fe71"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q5_K_M.gguf", "32c554c7d1338a23837ae39b3db482213a4a642f96df3b2619f516a2375d16cd"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q6_K.gguf", "6f394336500bb5409bd68c31aa9d75097b5e6104f6cf30c82a09a7b64df41faf"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q8_0.gguf", "f20e7ff1be28c283eeeb18fc895733791c56a5851d5cd3fe9691b7f7d12afa72"),
];

/// Pins whose publisher states no SHA-256 (Purfview's Whisper-Faster releases have no GitHub digest, NVIDIA's
/// manifest of cuDNN 8.9.7.29 lists only its CUDA 12 builds): checked by size and install record only.
#[cfg(test)]
const NO_PUBLISHED_SHA256: &[&str] = &[
    "https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r192.3_windows.zip",
    "https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r189.1_linux.zip",
    "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip",
    "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-8.9.7.29_cuda11-archive.tar.xz",
];

/// The SHA-256 the pin at `url` names: this table, then the transcribe.cpp builds and models of the shared binding,
/// then audio.cpp's server archives and the CUDA 12 wheels of the shared client.
pub fn sha256(url: &str) -> Option<&'static str> {
    if let Some((_, sha256)) = PINNED.iter().find(|(pinned, _)| *pinned == url) {
        return Some(sha256);
    }
    if let Some(download) = dub_tcpp::Build::ALL.iter().map(|build| build.download()).find(|download| download.url == url) {
        return Some(download.sha256);
    }
    if let Some(model) = dub_tcpp::MODELS.iter().find(|model| model.download.url == url) {
        return Some(model.download.sha256);
    }
    crate::acpp::BUILDS
        .iter()
        .filter_map(|build| build.server())
        .chain(acpp_server::asset::CUDA12_RUNTIME.iter().copied())
        .find(|asset| asset.url == url)
        .map(|asset| asset.sha256)
}

/// Whether `url` is a pin whose publisher states no SHA-256.
#[cfg(test)]
pub fn unpublished(url: &str) -> bool {
    NO_PUBLISHED_SHA256.contains(&url)
}

/// The SHA-256 of the file downloaded from `url`, checked against the pin: a file that differs is removed and the
/// download fails with both hashes.
pub fn check(url: &str, file: &Path) -> Result<String> {
    let got = crate::install_record::sha256_of(file)?;
    if let Some(pinned) = sha256(url).filter(|pinned| *pinned != got) {
        let _ = std::fs::remove_file(file);
        bail!("{}: its SHA-256 {got} differs from the pinned {pinned}; the file is removed, download it again", file.display());
    }
    Ok(got)
}

/// Whether a neighbour's file staged at `staged` is the pin's: its hash when the pin names one.
pub fn accepts(url: &str, staged: &Path) -> bool {
    match sha256(url) {
        Some(pinned) => crate::install_record::sha256_of(staged).is_ok_and(|got| got == pinned),
        None => true,
    }
}

/// The SHA-256 of the files a pick takes out of a pinned package, by the package's URL and the file's name, as the
/// package's own RECORD lists them: a file already on disk from another address is known by its bytes.
const PICKED: &[(&str, &str, &str)] = &[
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublas64_12.dll", "52ce5ba0ec5327d39be6021f0f9362d2ed4d64d7206f9b5afc06398d827b8e82"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublasLt64_12.dll", "71705a7cf0923f4ab034d0d2620bbfc989ff669ac3d79f868b7a7c597ca559c5"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "libcublas.so.12", "5757ab5839fb4f203ca47ecb336110d10f4a5606b1e097f195fbca89774569e2"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "libcublasLt.so.12", "2c9006a75c74b3bea2dc7ae2ec38ab038b0e45ea02cb4b717a915e8a5796acb1"),
];

/// The SHA-256 of the file `name` a pick takes out of the package at `url`, when the package lists it.
pub fn picked(url: &str, name: &str) -> Option<&'static str> {
    PICKED.iter().find(|(package, file, _)| *package == url && *file == name).map(|(_, _, sha256)| *sha256)
}

/// Whether `path` holds the bytes of `sha256`. A file is hashed once for its size and modification time, since the
/// models page asks every second.
pub fn holds(path: &Path, sha256: &str) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static HASHED: OnceLock<Mutex<HashMap<std::path::PathBuf, (u64, std::time::SystemTime, String)>>> = OnceLock::new();
    let Ok(meta) = std::fs::metadata(path) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    let hashed = HASHED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((len, at, got)) = hashed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(path) {
        if *len == meta.len() && *at == modified {
            return got == sha256;
        }
    }
    let got = match crate::install_record::sha256_of(path) {
        Ok(got) => got,
        Err(error) => {
            eprintln!("[ERROR] {} could not be hashed: {error:#}", path.display());
            return false;
        }
    };
    let same = got == sha256;
    hashed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(path.to_path_buf(), (meta.len(), modified, got));
    same
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picked_hash_is_of_a_file_a_download_picks() {
        let downloads: Vec<&crate::downloads::Asset> = crate::lyrics_sync::ASSETS.iter().chain(crate::engine_runtime::ASSETS).collect();
        for (url, name, sha256) in PICKED {
            assert_eq!(sha256.len(), 64, "{name}");
            if let Some(asset) = downloads.iter().find(|asset| asset.url == *url) {
                assert!(asset.pick.iter().any(|entry| crate::downloads::picked_file_name(entry) == *name), "{} does not pick {name}", asset.id);
            }
        }
        for asset in downloads.iter().filter(|asset| PICKED.iter().any(|(url, _, _)| *url == asset.url)) {
            for entry in asset.pick {
                assert!(picked(asset.url, crate::downloads::picked_file_name(entry)).is_some(), "{}: {entry} has no hash", asset.id);
            }
        }
    }

    #[test]
    fn a_file_is_known_by_its_bytes_and_hashed_again_once_it_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("cublas64_12.dll");
        std::fs::write(&file, b"today").unwrap();
        let today = crate::install_record::sha256_of(&file).unwrap();
        assert!(holds(&file, &today));
        assert!(holds(&file, &today), "the second look is the first one's hash");
        std::fs::write(&file, b"an earlier build").unwrap();
        assert!(!holds(&file, &today), "another size is hashed again");
        assert!(!holds(&root.path().join("missing.dll"), &today));
    }

    #[test]
    fn every_download_names_the_sha256_of_its_pin() {
        let mut urls: Vec<&str> = Vec::new();
        urls.extend(crate::lyrics_sync::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::engine_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend([crate::separation::MODEL.url, crate::separation::ROFORMER_MODEL.url]);
        urls.extend([true, false].into_iter().filter_map(crate::separation::Separator::roformer_engine).map(|asset| asset.url));
        urls.extend(crate::acpp::BUILDS.into_iter().flat_map(crate::acpp::assets).map(|asset| asset.url));
        urls.extend(crate::midi::SIZES.iter().flat_map(crate::midi::weight_assets).map(|asset| asset.url));
        urls.extend(crate::training::pack().iter().chain(crate::training::listen_pack()).map(|asset| asset.url));
        let missing: Vec<&str> = urls.into_iter().filter(|url| sha256(url).is_none() && !unpublished(url)).collect();
        assert!(missing.is_empty(), "pins without a SHA-256: {missing:#?}");
        for (url, sha256) in PINNED {
            assert!(sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()), "{url}: {sha256}");
            assert!(!unpublished(url), "{url} is both pinned and unpublished");
        }
    }

    #[test]
    fn a_file_that_is_not_the_pins_is_removed() {
        let dir = std::env::temp_dir().join(format!("pin-sha-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        std::fs::write(&file, b"not the pinned config").unwrap();
        let url = "https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/config.json";
        let error = check(url, &file).unwrap_err().to_string();
        assert!(error.contains("differs from the pinned a73a28cd"), "{error}");
        assert!(!file.exists());
        assert!(!accepts(url, &file));

        std::fs::write(&file, b"anything").unwrap();
        assert!(check("https://example.invalid/unpinned.bin", &file).is_ok(), "a url without a pin is not refused here");
        std::fs::remove_dir_all(&dir).ok();
    }
}
