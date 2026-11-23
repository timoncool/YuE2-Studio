//! The SHA-256 of every pinned download, as its publisher states it: GitHub's release asset digest, the LFS object
//! id on Hugging Face (and, for a small file kept in git there or on GitHub, the hash of the file whose git object id
//! is the pinned revision's), NVIDIA's redistributable manifests, PyPI's file hashes. A downloaded file whose hash
//! differs is removed and the download fails; a neighbour's file is taken only with the pin's hash.

use std::path::Path;

use anyhow::{bail, Result};

const PINNED: &[(&str, &str)] = &[
    (crate::acpp::SOXR_URL, "1c2177da4871f1713b666f576f83be649980b66d783723d9572a0ffffe67fe26"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip", "69d0ad8dc3a1be66f01a748a8206d0ceaafa56939474663fbe790dc3e91d2009"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661"),
    ("https://files.pythonhosted.org/packages/76/bf/3fea3d1c6262bded26ae00e3106432d63235954965d7785f5051ac146651/nvidia_cufft-12.4.0.43-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "0e8385013596b112d29c9ce8c63dc575b308d77636c7169104e18714f03961a8"),
    ("https://files.pythonhosted.org/packages/95/f4/61e6996dd20481ee834f57a8e9dca28b1869366a135e0d42e2aa8493bdd4/nvidia_cufft_cu12-11.4.1.4-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "c67884f2a7d276b4b80eb56a79322a95df592ae5e765cf1243693365ccab4e28"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cuda13-x64.tar.gz", "807f59113f17eb706bc7289155bdf507d05ae01650a5c2ccd9685f5574e9cd04"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cpu-x64.tar.gz", "65eb3cf26dfed0a23f01906a9f179ae7ab503f2bdac4f411e49ec902a5addafd"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-macos-arm64.tar.xz", "d0fc45181d31dcceea99c3828378f4a7376e52e7fdbc43e29a07e806785b4c9a"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.2/BSRoformer-windows-cuda13-x64.zip", "151d8d38a2159107c17c9b528261a0be89d5f85a73110594056e938697b64d1b"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.2/BSRoformer-windows-cpu-x64.zip", "d2fa6bbfd7174edb23114f1774aa9039c59207e8163f2af1adb1a0e573e1c2cf"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.2/BSRoformer-windows-cuda12-x64.zip", "a7df43938ce9aebe6106768214076b4194af40d0ca9dfa98ea70252903ebecc2"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.2/BSRoformer-windows-vulkan-x64.zip", "12ca40c094e1feb69b4c9de512c694681ed7bed55260c6d86bd1e6755ae58755"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cuda12-x64.tar.gz", "82540c02b5a9730137b0059ffda8c0c79b589a649af554dff1f88867a6c7444d"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-vulkan-x64.tar.gz", "d0b6acc7b4b05c453bbfe99ea8154251fde4d96b2edd957fdab6e596f7a64927"),
    ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz", "c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9"),
    ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz", "2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f"),
    ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip", "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-75923e70/ace-caption-windows-x64.zip", "254af78ec1254ad2d141afb12f5dc20014dcbbb2387eed1b63a975d5ee46a52a"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-75923e70/hotstep-tools-cuda-windows-x64.zip", "f423135405e51ed861b51ad1fa8dc108d891914951915eb54e18c03b8271331a"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-75923e70-xcr0/ace-caption-xcr0-windows-x64.zip", "934bb5841e2deaa12db73e6cd29ac3f205e1a3e10e0a19a9809113fc989dbee5"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-75923e70-xcr0/hotstep-tools-xcr0-cuda-windows-x64.zip", "c7fd3d093a1ed769d7d6159cd4c8fadf70105d360470597cc96067cbcba9893b"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-479cbeb8-xcr0/hotstep-tools-479cbeb8-xcr0-cuda-windows-x64.zip", "892e370c877232edf0bb0228eac3ed54eec8c264d49f2457d3534cca5d2ab0ff"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-479cbeb8-xcr0/ace-caption-479cbeb8-xcr0-windows-x64.zip", "6423f74c630f0e240983748ea16d8670e241bcc834f73d9db9399a45d118b33c"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-trainer-75923e70-cuda12/hotstep-trainer-cuda12-backend-windows-x64.zip", "63bb122cb2634146a578be1635f4456a218679a3b500532a30d0c04d0e54383f"),
];

/// The SHA-256 the pin at `url` names: this table, then the transcribe.cpp builds and models of the shared binding,
/// then audio.cpp's server archives and the CUDA 12 wheels of the shared client.
pub fn sha256(url: &str) -> Option<&'static str> {
    if let Some(sha) = crate::components::sha256(url) {
        return Some(sha);
    }
    if let Some(sha) = crate::engine_hip::archive_sha(url)
        .or_else(|| crate::native_music_runtime::sha256(url))
    {
        return Some(sha);
    }
    if let Some(sha) = crate::hotstep_cover_assets::sha256(url).or_else(|| crate::hotstep_sa3_train_assets::sha256(url)) { return Some(sha); }
    if let Some((_, sha256)) = PINNED.iter().find(|(pinned, _)| *pinned == url) {
        return Some(sha256);
    }
    if let Some(download) = dub_tcpp::Build::ALL.iter().map(|build| build.download()).find(|download| download.url == url) {
        return Some(download.sha256);
    }
    if let Some(sha256) = crate::community::sha256(url) {
        return Some(sha256);
    }
    if let Some(sha256) = crate::rvc_burn_install::sha256(url).or_else(|| crate::rvc_fast_install::sha256(url)) {
        return Some(sha256);
    }
    crate::acpp::BUILDS
        .iter()
        .filter_map(|build| build.server())
        .chain(acpp_server::asset::CUDA12_RUNTIME.iter().copied())
        .find(|asset| asset.url == url)
        .map(|asset| asset.sha256)
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
    (crate::acpp::SOXR_URL, "soxr.dll", "3614f7703a5e48dce72cc0726390782287779195149657f87963792f37e76791"),
    (crate::acpp::SOXR_URL, "soxr.txt", "f2f118b9029ec1871b953639ecc46651b2fc7b62e295e6cf3ef2ac4c9a058b33"),
    ("https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl", "cudart64_12.dll", "760c38928bbe5759f7b31ed6692599eb7ec83cedd5702e84c2b72028a89837e1"),
    ("https://files.pythonhosted.org/packages/20/ee/29955203338515b940bd4f60ffdbc073428f25ef9bfbce44c9a066aedc5c/nvidia_cufft_cu12-11.4.1.4-py3-none-win_amd64.whl", "cufft64_11.dll", "5b90655ca9cdf5d91cbdf2c72b995ec937eed0aef2d6954370c711b45c929b35"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublas64_12.dll", "52ce5ba0ec5327d39be6021f0f9362d2ed4d64d7206f9b5afc06398d827b8e82"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublasLt64_12.dll", "71705a7cf0923f4ab034d0d2620bbfc989ff669ac3d79f868b7a7c597ca559c5"),
];

/// The SHA-256 of the file `name` a pick takes out of the package at `url`, when the package lists it.
pub fn picked(url: &str, name: &str) -> Option<&'static str> {
    if let Some(list) = crate::components::contents(url) {
        return list.iter().find(|(file, _)| *file == name).map(|(_, sha256)| *sha256);
    }
    PICKED.iter().find(|(package, file, _)| *package == url && *file == name).map(|(_, _, sha256)| *sha256)
}

/// Libraries required to run a fully unpacked archive without downloading it again.
pub(crate) fn runtime_libraries(url: &str) -> &'static [(&'static str, &'static str)] {
    if let Some(list) = crate::components::contents(url) {
        return list;
    }
    match url {
        "https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-cpu-x64.zip" => &[
            ("ggml-cpu-alderlake.dll", "e0cbca54dbbb1ab40336df0ddc3d1d5737b0d77c9ffc725bebb2f0155e65e7b1"),
            ("ggml-cpu-cannonlake.dll", "a4b560a4141c05ea824a3fc679cbb70abe2b3b4d7969107d4056effee0d13bd9"),
            ("ggml-cpu-cascadelake.dll", "53e758a6a6c130a22c93912a7ba3de5342fb4ab08c82031146a66c4228f51930"),
            ("ggml-cpu-haswell.dll", "82de5989496cf8453522066fa90f82449676f5b75d837707d07f4b74d06f32a1"),
            ("ggml-cpu-icelake.dll", "a28f9d4896df07c231651f909c33584b0ff9bb379b3194a90b2fed77a2494069"),
            ("ggml-cpu-sandybridge.dll", "b288b07f3058eb28a679e3493f17b10f76aaab8afae2e720f676c36945c6ebf1"),
            ("ggml-cpu-skylakex.dll", "8e1f896b413e2c47665b47bfc15674798c5614901f2326e10bc3a4fe05e99de2"),
            ("ggml-cpu-sse42.dll", "cadb7f748efcea4990e522ef1bd8d3a30a2ec57ac362b5a1fca1e7b1fa419803"),
            ("ggml-cpu-x64.dll", "77b72dc88cc88f488a78c4049125d0faa3ec87d938980d6931aa90feb48aefce"),
            ("transcribe-ggml-base.dll", "a2202e36b7bab534a88015c39bb5c20a131a4a9b611eca3105b9a7b82ba2a832"),
            ("transcribe-ggml.dll", "32c88438baab7efe9cedad946e8f33cc439fa7317daffda5ff23f23f8a988ebd"),
            ("transcribe.dll", "8df55e838eb9c958024811b995eb511eb8fbc12d45451afdcde660f75e84dc6a"),
        ],
        "https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-cuda12-x64.zip" => &[
            ("ggml-cpu-alderlake.dll", "c903ed71e49ccdbae466c1dec036293d425d0669f2eb7b849fb7a517789435c9"),
            ("ggml-cpu-cannonlake.dll", "3a581dccb2ab0700dba39333c1e478e578baed8f734b879c5bfbee142bebc445"),
            ("ggml-cpu-cascadelake.dll", "e815ce4686c73483cf63adb3f0bdc6b2786b05f6116c14378d4c9caf7f1b7859"),
            ("ggml-cpu-haswell.dll", "4a815420bfdde92f6f8f3dbe93214265bc31e2df110b25cfc7585c4f7884f35b"),
            ("ggml-cpu-icelake.dll", "1e4259fb0e2c1d4730b6dfad31ebbbf539c798e000ddc0ffd01e6c409e41dabc"),
            ("ggml-cpu-sandybridge.dll", "a690d2f0333e0fd423a1ede57701f3cd83d5fff1c0d8d3d399a34b92bfc9fb80"),
            ("ggml-cpu-skylakex.dll", "4b8e1dc9241f483a33f136bfa2b2c6f160fb2a0a749acf749fe105fef2d84971"),
            ("ggml-cpu-sse42.dll", "305f4d9d4a4ad797f032c9320be0eed1d43c4acb4b14f5105a2cbf77ded096b9"),
            ("ggml-cpu-x64.dll", "d774ac4774e7d9747d9207176243c44d65fd1924853e7b1eaa7d679cb6559295"),
            ("ggml-cuda.dll", "71d5a82d3d48e1c674ac156b0bac04a299a6dd3920adb914d92dc1b3dbb0b51f"),
            ("transcribe-ggml-base.dll", "7f6da901a1f169c714a3dd35204122c02db856c4ffb64f758e880bf9125756b7"),
            ("transcribe-ggml.dll", "7eba746044a1399a4847f711fa859d1d10caea0d107d4f8c80d9bc6d0be88169"),
            ("transcribe.dll", "7cfdfc0cda4a2012c61608bd290424e8286beb048614e2d820e7a820122f3e68"),
        ],
        "https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-cuda13-x64.zip" => &[
            ("ggml-cpu-alderlake.dll", "a99ab1f0a7ea59a3d843b390366d86ee04532282320881e9fd37a4b870658002"),
            ("ggml-cpu-cannonlake.dll", "1522b8592c3a5bf2f6ca3b29a0432bd45d2cd2d123dbf21cdccc0e9382b801ec"),
            ("ggml-cpu-cascadelake.dll", "c79e1e2e3d31dcab1894fab1fef4bebe38bfea40dd8c193fe7a84d842fdbd81b"),
            ("ggml-cpu-haswell.dll", "2ccb2847169284e0424de8974817c0ae5ddee333f9bab462689dff018e91e7e4"),
            ("ggml-cpu-icelake.dll", "87fdc1b3fde49611991617a45471991afd52bd17b312bf91d7e46647b4817106"),
            ("ggml-cpu-sandybridge.dll", "ed395d4d7f94ce14037b3170ffa99c67581a8e3fcf0cf0772abdb35c2482e0dd"),
            ("ggml-cpu-skylakex.dll", "507e8cbc663f2ef1e8a4d88d182e25de42b3222c4879d181f3fe0e71236bbcae"),
            ("ggml-cpu-sse42.dll", "3d37ae65a8011ca1edeaff92b265f373b548244f87f9c9a9bd79c85fa21fd746"),
            ("ggml-cpu-x64.dll", "4460f21697a28174f759dde52da19796dc3abb7fcbd419850314ab89776c6a8f"),
            ("ggml-cuda.dll", "cc8d7a2b667d273bbff03369106cf9d688311d2715f67b2c808cac0bbaf960dc"),
            ("transcribe-ggml-base.dll", "51f422ac39e152e3c6ac0526d573138db2e00e2c7afe36266ee88b1de5e189e6"),
            ("transcribe-ggml.dll", "e417d6707cca79f825bd4bcb8c2b72138624480e527492d5cb224fafd5a77f32"),
            ("transcribe.dll", "3ef1a44f8992093edc34fa55754096b19e367f092a46621c79499946c1db782c"),
        ],
        "https://github.com/timoncool/transcribe.cpp/releases/download/studio-engine-274d105e/transcribe-windows-vulkan-x64.zip" => &[
            ("ggml-cpu-alderlake.dll", "065399f091f1e79dee7acc4d67ba9afad14dab29815aa9bb8e0f710bfd3aac06"),
            ("ggml-cpu-cannonlake.dll", "0d585cc14c6ae8511df8da2341712d79ebe364c530a5cdfd4bb0b8927183f29c"),
            ("ggml-cpu-cascadelake.dll", "45a91e6f48f2cc0008d1372eb5ec137634e98e0df1103c1ff316c25b71e11a01"),
            ("ggml-cpu-haswell.dll", "2156c2367721f36909c0b48bb9fdfa88f09025b6993bd55a5ce56282d7a41f47"),
            ("ggml-cpu-icelake.dll", "41b1772592451d88647442a666b0aac42493208aeb1527768ea073ce5d78f6c0"),
            ("ggml-cpu-sandybridge.dll", "3dbfdd15b7f03fa201afc4056d013e43d61e86089f3f15c7e5c07c1c602f7eb6"),
            ("ggml-cpu-skylakex.dll", "b7220f63c324ae8a8b119aa2ed1d56e54d0b46a15d17f1e95904ba306cb2be1a"),
            ("ggml-cpu-sse42.dll", "472aa1b04c30dceac08ff2060115ce17a7d7d6bc181f03a5bea5bddc7565eac9"),
            ("ggml-cpu-x64.dll", "f46f7a03976c22bda585ada21d7737d702653fd797d39b486ebc69e69bef81f1"),
            ("ggml-vulkan.dll", "24234a7204d2a3a30c2e236fbc03035ce20f2be53b9e1f8bdb89ce6079d0d94e"),
            ("transcribe-ggml-base.dll", "8076749dff87fc84ff1267caa6aae79782333fb788288addc68bfd2479c5ef50"),
            ("transcribe-ggml.dll", "ab4398ed5ab6430f62cc04ac303b1e0d1cc5e728d8501655a3aaf538d4dc4166"),
            ("transcribe.dll", "de70373e3f7fadeb6c88305380141804230d64452a403f44eca6a9017fc2ff6c"),
        ],
        _ => &[],
    }
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
        let downloads: Vec<&crate::downloads::Asset> = crate::lyrics_sync::assets().into_iter().chain(crate::engine_runtime::assets()).collect();
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
        urls.extend(crate::lyrics_sync::assets().into_iter().map(|asset| asset.url));
        urls.extend(crate::engine_runtime::assets().iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::assets().into_iter().map(|asset| asset.url));
        urls.extend([crate::separation::model().url, crate::separation::roformer_model().url]);
        urls.extend(crate::separation::RoformerBuild::ALL.into_iter().filter_map(crate::separation::roformer_asset).map(|asset| asset.url));
        urls.extend(crate::acpp::BUILDS.into_iter().flat_map(crate::acpp::assets).map(|asset| asset.url));
        urls.extend(crate::midi::SIZES.iter().flat_map(crate::midi::weight_assets).map(|asset| asset.url));
        urls.extend(crate::training::pack().iter().chain(crate::training::listen_pack()).map(|asset| asset.url));
        let missing: Vec<&str> = urls.into_iter().filter(|url| sha256(url).is_none()).collect();
        assert!(missing.is_empty(), "pins without a SHA-256: {missing:#?}");
        for (url, sha256) in PINNED {
            assert!(sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()), "{url}: {sha256}");
        }
    }

    #[test]
    fn no_download_follows_a_branch() {
        let mut urls: Vec<&str> = Vec::new();
        urls.extend(crate::lyrics_sync::assets().into_iter().map(|asset| asset.url));
        urls.extend(crate::acpp::BUILDS.into_iter().flat_map(crate::acpp::assets).map(|asset| asset.url));
        urls.extend(crate::engine_runtime::assets().iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::assets().into_iter().map(|asset| asset.url));
        urls.extend([crate::separation::model().url, crate::separation::roformer_model().url]);
        urls.extend(crate::separation::RoformerBuild::ALL.into_iter().filter_map(crate::separation::roformer_asset).map(|asset| asset.url));
        urls.extend(crate::training::pack().iter().map(|asset| asset.url));
        urls.extend(crate::training::listen_pack().iter().map(|asset| asset.url));
        urls.extend(crate::midi::SIZES.iter().flat_map(crate::midi::weight_assets).map(|asset| asset.url));
        urls.extend(crate::community::catalog().iter().flat_map(|model| &model.files).map(|file| file.asset.url));
        let branches: Vec<&str> = urls.into_iter().filter(|url| url.contains("/resolve/main/") || url.contains("/raw/main/")).collect();
        assert!(branches.is_empty(), "downloads that follow a branch instead of a pinned revision: {branches:#?}");
    }

    #[test]
    fn a_file_that_is_not_the_pins_is_removed() {
        let dir = std::env::temp_dir().join(format!("pin-sha-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        std::fs::write(&file, b"not the pinned config").unwrap();
        let url = "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json";
        let error = check(url, &file).unwrap_err().to_string();
        assert!(error.contains("differs from the pinned 9d823d0e"), "{error}");
        assert!(!file.exists());
        assert!(!accepts(url, &file));

        std::fs::write(&file, b"anything").unwrap();
        assert!(check("https://example.invalid/unpinned.bin", &file).is_ok(), "a url without a pin is not refused here");
        std::fs::remove_dir_all(&dir).ok();
    }
}
