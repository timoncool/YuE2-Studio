//! Community YuE2 models: fine-tunes published by others that the studio downloads, by a pinned revision, into the
//! folder of the person's own models (`models/yue2-cpp/own`), where they are chosen like any own file
//! (`config/community-models.json`).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::downloads::{Asset, AssetKind};

#[derive(Deserialize)]
struct CatalogFile {
    repository: String,
    revision: String,
    models: Vec<ModelFile>,
}

#[derive(Deserialize)]
struct ModelFile {
    id: String,
    author: String,
    page: String,
    license: String,
    name: HashMap<String, String>,
    files: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct FileEntry {
    quant: String,
    file: String,
    bytes: u64,
    sha256: String,
}

/// One community model, its names by language and its downloads by quantisation.
pub struct Model {
    pub id: &'static str,
    pub author: &'static str,
    pub page: &'static str,
    pub license: &'static str,
    pub name: HashMap<String, String>,
    pub files: Vec<File>,
}

/// One quantisation of a community model: the download into the own-models folder and the SHA-256 it is checked by.
pub struct File {
    pub quant: &'static str,
    pub asset: Asset,
    pub sha256: &'static str,
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The catalog, in its file's order: the authors' choice first.
pub fn catalog() -> &'static [Model] {
    static CATALOG: OnceLock<Vec<Model>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let parsed: CatalogFile = serde_json::from_str(include_str!("../../../config/community-models.json")).expect("config/community-models.json is valid");
        parsed
            .models
            .into_iter()
            .map(|model| Model {
                id: leak(model.id),
                author: leak(model.author),
                page: leak(model.page),
                license: leak(model.license),
                name: model.name,
                files: model
                    .files
                    .into_iter()
                    .map(|entry| {
                        let url = format!("https://huggingface.co/{}/resolve/{}/{}", parsed.repository, parsed.revision, entry.file);
                        let file = leak(entry.file);
                        File {
                            quant: leak(entry.quant),
                            sha256: leak(entry.sha256),
                            asset: Asset {
                                id: file,
                                label: file,
                                kind: AssetKind::Model,
                                url: leak(url),
                                relative_path: file,
                                bytes: entry.bytes,
                                unzip_into: None,
                                marker: "",
                                pick: &[],
                                vram_gb: None,
                                note: "A YuE2 fine-tune of the community, sung in place of the set's model.",
                            },
                        }
                    })
                    .collect(),
            })
            .collect()
    })
}

/// The SHA-256 a community download is pinned to, by its URL.
pub fn sha256(url: &str) -> Option<&'static str> {
    catalog().iter().flat_map(|model| &model.files).find(|file| file.asset.url == url).map(|file| file.sha256)
}

/// The download of `id` in `quant`, or in the quantisation the card holds when none is named: Q8_0 from 8 GB of
/// video memory, Q6_K below, as the model sets.
pub fn file(id: &str, quant: Option<&str>, vram_gb: f64) -> Option<&'static File> {
    let model = catalog().iter().find(|model| model.id == id)?;
    let wanted = quant.unwrap_or(if vram_gb >= 8.0 { "Q8_0" } else { "Q6_K" });
    model.files.iter().find(|file| file.quant.eq_ignore_ascii_case(wanted))
}

/// What choosing community model `id` in `quant` (none: the card's) does, given the GGUF files of the own-models
/// folder: the file is there and is chosen, or it is downloaded first; an id or quantisation the catalog has not got
/// is refused.
#[derive(Debug, PartialEq)]
pub enum Choice {
    Select(&'static str),
    /// The file name the download lands as.
    Download(&'static str),
    Unknown,
}

pub fn choice(on_disk: &[String], id: &str, quant: Option<&str>, vram_gb: f64) -> Choice {
    match file(id, quant, vram_gb) {
        None => Choice::Unknown,
        Some(file) if on_disk.iter().any(|name| name == file.asset.relative_path) => Choice::Select(file.asset.relative_path),
        Some(file) => Choice::Download(file.asset.relative_path),
    }
}

/// The name of the community model a file of the own-models folder is, in each language; none for a file of the
/// person's own.
pub fn name_of(file_name: &str) -> Option<&'static HashMap<String, String>> {
    catalog().iter().find(|model| model.files.iter().any(|file| file.asset.relative_path == file_name)).map(|model| &model.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_reads_with_a_pinned_revision_and_a_digest_for_every_file() {
        let models = catalog();
        assert_eq!(models.len(), 9);
        assert_eq!(models[0].id, "rs-tw-2.0x0.75", "the authors' choice comes first");
        for model in models {
            for language in ["en", "ru", "zh", "ja", "ko"] {
                assert!(model.name.get(language).is_some_and(|name| !name.is_empty()), "{} has no {language} name", model.id);
            }
            assert_eq!(model.files.len(), 2, "{}", model.id);
            for file in &model.files {
                assert!(file.asset.url.contains("/resolve/b371be93fe7320c7084813b4f5967a6820756a06/"), "{}", file.asset.url);
                assert!(file.sha256.len() == 64 && file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()), "{}", file.asset.id);
                assert_eq!(sha256(file.asset.url), Some(file.sha256));
                assert!(file.asset.relative_path.ends_with(".gguf") && !file.asset.relative_path.contains('/'), "lands in the own-models folder itself");
            }
        }
    }

    #[test]
    fn the_card_picks_the_quantisation_unless_one_is_named() {
        assert_eq!(file("rs-tw-2.0x0.75", None, 24.0).map(|file| file.quant), Some("Q8_0"));
        assert_eq!(file("rs-tw-2.0x0.75", None, 6.0).map(|file| file.quant), Some("Q6_K"));
        assert_eq!(file("rs-tw-2.0x0.75", Some("Q6_K"), 24.0).map(|file| file.asset.relative_path), Some("YuE2-3B-RS-TW-2.0x0.75-Q6_K.gguf"));
        assert!(file("no-such-model", None, 24.0).is_none());
    }

    #[test]
    fn a_catalog_entry_not_on_disk_is_downloaded_and_one_there_is_chosen() {
        let none: Vec<String> = Vec::new();
        assert_eq!(choice(&none, "rs-tw-2.0x0.75", Some("Q8_0"), 24.0), Choice::Download("YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf"));
        let there = vec!["mine.gguf".to_string(), "YuE2-3B-RS-TW-2.0x0.75-Q6_K.gguf".to_string()];
        assert_eq!(choice(&there, "rs-tw-2.0x0.75", None, 6.0), Choice::Select("YuE2-3B-RS-TW-2.0x0.75-Q6_K.gguf"));
        assert_eq!(choice(&there, "no-such-model", None, 24.0), Choice::Unknown);
        assert_eq!(choice(&there, "rs-tw-2.0x0.75", Some("Q4_0"), 24.0), Choice::Unknown);
        assert_eq!(name_of("YuE2-3B-RS-TW-2.0x0.75-Q6_K.gguf").and_then(|name| name.get("en")).map(String::as_str), Some("Ruach Studio TW 2.0×0.75 (the authors' choice)"));
        assert!(name_of("mine.gguf").is_none());
    }
}
