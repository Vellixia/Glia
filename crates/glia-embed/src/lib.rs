//! Local sentence embeddings for Glia.
//!
//! Pure-Rust, air-gapped. Bundles MiniLM-L6-v2 (384-dim) via `rust-embed`.
//! No network. No C++ toolchain. Tokenize → forward → mean-pool → L2-normalize.

use std::sync::Mutex;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use rust_embed::RustEmbed;
use tokenizers::{Encoding, tokenizer::Tokenizer};

/// Embedded model assets (weights + tokenizer + config).
#[derive(RustEmbed)]
#[folder = "embed/"]
struct ModelAssets;

/// Errors from embedding.
#[derive(thiserror::Error, Debug)]
pub enum EmbedError {
    /// Required model asset (weights, tokenizer, or config) is missing from the binary.
    #[error("missing model asset: {0}")]
    MissingAsset(&'static str),
    /// Tokenizer init or encode failed.
    #[error("tokenizer load: {0}")]
    Tokenizer(String),
    /// `config.json` failed to deserialize into a `candle_transformers::models::bert::Config`.
    #[error("config parse: {0}")]
    Config(String),
    /// Candle tensor / forward error.
    #[error("candle: {0}")]
    Candle(#[from] candle_core::Error),
    /// Tokenizer produced zero tokens (defensive; should not happen in practice).
    #[error("embed empty")]
    Empty,
}

/// 384-dim normalized embedding.
pub type Vector = Vec<f32>;

/// Local embedder. Loads model on first `new()`. Thread-safe.
pub struct Embedder {
    model: Mutex<BertModel>,
    tokenizer: Mutex<Tokenizer>,
    device: Device,
}

impl Embedder {
    /// Load model from embedded assets. CPU device.
    pub fn new() -> Result<Self, EmbedError> {
        Self::with_device(Device::Cpu)
    }

    /// Returns `None` if the model is missing or fails to load for any
    /// reason. Used by integration tests in other crates to skip cleanly
    /// when the model assets are not present (CI, fresh clone).
    pub fn try_new() -> Option<Self> {
        Self::new().ok()
    }

    /// Load model on a specific device (test seam).
    pub fn with_device(device: Device) -> Result<Self, EmbedError> {
        let weights = load_asset("model.safetensors")?;
        let tokenizer_bytes = load_asset("tokenizer.json")?;
        let config_bytes = load_asset("config.json")?;

        let config: Config =
            serde_json::from_slice(&config_bytes).map_err(|e| EmbedError::Config(e.to_string()))?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)?;
        let model = BertModel::load(vb, &config)?;

        let tokenizer = Tokenizer::from_bytes(&tokenizer_bytes)
            .map_err(|e| EmbedError::Tokenizer(e.to_string()))?;

        Ok(Self {
            model: Mutex::new(model),
            tokenizer: Mutex::new(tokenizer),
            device,
        })
    }

    /// Embed a single string. Returns 384-dim L2-normalized vector.
    pub fn embed(&self, text: &str) -> Result<Vector, EmbedError> {
        let encoding = self.encode(text)?;
        self.embed_encoding(&encoding)
    }

    fn encode(&self, text: &str) -> Result<Encoding, EmbedError> {
        let tok = self.tokenizer.lock().expect("tokenizer poisoned");
        tok.encode(text, true)
            .map_err(|e| EmbedError::Tokenizer(e.to_string()))
    }

    fn embed_encoding(&self, enc: &Encoding) -> Result<Vector, EmbedError> {
        let ids = enc.get_ids();
        let mask = enc.get_attention_mask();
        let type_ids = enc.get_type_ids();
        if ids.is_empty() {
            return Err(EmbedError::Empty);
        }
        let len = ids.len();

        let input_ids = Tensor::new(ids, &self.device)?.reshape((1, len))?;
        let attention_mask = Tensor::new(mask, &self.device)?.reshape((1, len))?;
        let token_type_ids = Tensor::new(type_ids, &self.device)?.reshape((1, len))?;

        let model = self.model.lock().expect("model poisoned");
        let hidden = model.forward(&input_ids, &attention_mask, Some(&token_type_ids))?;
        drop(model);

        // Mean-pool over tokens with attention mask, then L2-normalize.
        let mask_f = attention_mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let summed = hidden.broadcast_mul(&mask_f)?.sum(1)?;
        let counts = mask_f.sum(1)?.clamp(1e-9, f32::INFINITY)?;
        let pooled = summed.broadcast_div(&counts)?;

        let norm = pooled
            .sqr()?
            .sum_keepdim(1)?
            .sqrt()?
            .clamp(1e-12, f32::INFINITY)?;
        let normalized = pooled.broadcast_div(&norm)?;

        let vec: Vector = normalized.squeeze(0)?.to_vec1()?;
        Ok(vec)
    }
}

fn load_asset(name: &'static str) -> Result<Vec<u8>, EmbedError> {
    let file = ModelAssets::get(name).ok_or(EmbedError::MissingAsset(name))?;
    Ok(file.data.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cos(a: &Vector, b: &Vector) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }

    /// Model assets are gitignored. Skip embed tests if missing so CI does
    /// not need network or LFS at build time. Local devs should run
    /// `./scripts/fetch-embed-model.sh` (or place the three files under
    /// `crates/glia-embed/embed/`) before running `cargo test -p glia-embed`.
    fn try_embedder() -> Option<Embedder> {
        match Embedder::new() {
            Ok(e) => Some(e),
            Err(EmbedError::MissingAsset(_)) => {
                eprintln!(
                    "skipping glia-embed test: model asset not present \
                     (see crates/glia-embed/embed/README.md)"
                );
                None
            }
            Err(e) => panic!("unexpected embed error: {e}"),
        }
    }

    #[test]
    fn loads_and_embeds() {
        let Some(e) = try_embedder() else { return };
        let v = e.embed("hello world").expect("embed");
        assert_eq!(v.len(), 384);
    }

    #[test]
    fn normalized_unit_length() {
        let Some(e) = try_embedder() else { return };
        let v = e.embed("rust embedder").expect("embed");
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-3, "expected unit norm, got {n}");
    }

    #[test]
    fn same_text_cosine_near_one() {
        let Some(e) = try_embedder() else { return };
        let a = e.embed("the cat sat on the mat").expect("a");
        let b = e.embed("the cat sat on the mat").expect("b");
        let s = cos(&a, &b);
        assert!(s > 0.99, "expected ~1.0, got {s}");
    }

    #[test]
    fn similar_text_higher_than_unrelated() {
        let Some(e) = try_embedder() else { return };
        let cats_a = e.embed("a cat is a small animal").expect("a");
        let cats_b = e.embed("kittens are young cats").expect("b");
        let rocket = e.embed("rocket launches into orbit").expect("c");
        let sim = cos(&cats_a, &cats_b);
        let diss = cos(&cats_a, &rocket);
        assert!(sim > diss, "sim={sim} should exceed diss={diss}");
        assert!(sim > 0.5, "sim={sim} should be > 0.5");
    }

    #[test]
    fn whitespace_input_still_embeds() {
        let Some(e) = try_embedder() else { return };
        let v = e.embed("   ").expect("embed");
        assert_eq!(v.len(), 384);
    }
}
