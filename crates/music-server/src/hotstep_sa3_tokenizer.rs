use anyhow::{Context, Result};
use std::path::Path;
use tokenizers::{Tokenizer, TruncationParams};

const TOKEN_COUNT: usize = 256;

pub(super) fn encode(models: &Path, prompt: &str) -> Result<(String, usize)> {
    let path = models.join("onnx/sa3/tokenizer.json");
    let mut tokenizer = Tokenizer::from_file(&path)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("Read HOT-Step SA3 tokenizer at {}", path.display()))?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: TOKEN_COUNT,
            ..Default::default()
        }))
        .map_err(anyhow::Error::msg)?;
    tokenizer.with_padding(None);
    let pad = tokenizer
        .token_to_id("<pad>")
        .context("HOT-Step SA3 tokenizer has no <pad> token")?;
    let encoding = tokenizer.encode(prompt, true).map_err(anyhow::Error::msg)?;
    let count = encoding.len();
    anyhow::ensure!(count > 0, "HOT-Step SA3 requires a nonempty prompt");
    let mut tokens = encoding.get_ids().to_vec();
    tokens.resize(TOKEN_COUNT, pad);
    Ok((
        tokens
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(","),
        count,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires the SHA-pinned SA3 Medium generation pack"]
    fn matches_the_stock_hotstep_client() {
        let models = std::env::var_os("HOTSTEP_SA3_TEST_MODELS").expect("HOTSTEP_SA3_TEST_MODELS");
        let (csv, count) = super::encode(
            std::path::Path::new(&models),
            "Instrumental pop, clean piano and drums with a steady beat.",
        )
        .unwrap();
        let ids = csv
            .split(',')
            .map(|id| id.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(count, 12);
        assert_eq!(ids.len(), 256);
        assert_eq!(
            &ids[..count],
            &[
                156535, 2463, 235269, 3903, 16748, 578, 38260, 675, 476, 22358, 10270, 235265
            ]
        );
        assert!(ids[count..].iter().all(|id| *id == 0));
    }
}
