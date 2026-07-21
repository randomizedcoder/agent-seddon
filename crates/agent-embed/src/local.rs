//! `LocalEmbedder` — a dependency-free, deterministic feature-hashing embedder.

use agent_core::{Embedder, Result};
use async_trait::async_trait;

/// Feature-hashing embedder: hashes word tokens + character trigrams into a
/// fixed-dimensional, L2-normalised vector. Deterministic and hermetic.
pub struct LocalEmbedder {
    dims: usize,
}

impl LocalEmbedder {
    /// `dims` is clamped to a sane minimum so the hashing space isn't degenerate.
    pub fn new(dims: usize) -> Self {
        Self { dims: dims.max(16) }
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dims];
        let lower = text.to_lowercase();
        for token in lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
        {
            add_feature(&mut v, token.as_bytes());
            // Character trigrams give some morphological overlap ("retry"/"retries")
            // beyond exact-token match.
            let bytes = token.as_bytes();
            if bytes.len() >= 3 {
                for w in bytes.windows(3) {
                    add_feature(&mut v, w);
                }
            }
        }
        l2_normalize(&mut v);
        v
    }
}

impl Default for LocalEmbedder {
    /// A 256-dimensional embedder — plenty of hashing space for repo-sized corpora.
    fn default() -> Self {
        Self::new(256)
    }
}

#[async_trait]
impl Embedder for LocalEmbedder {
    fn dimensions(&self) -> usize {
        self.dims
    }
    fn max_batch(&self) -> usize {
        64
    }
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed_one(text))
    }
    async fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}

/// Add a **signed** feature (the hash's high bit picks the sign, reducing the
/// collision bias of unsigned feature hashing).
fn add_feature(v: &mut [f32], key: &[u8]) {
    let h = fnv1a(key);
    let idx = (h % v.len() as u64) as usize;
    let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
    v[idx] += sign;
}

/// FNV-1a — a fast, deterministic, cross-platform hash (no external dep).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::cosine_similarity;

    #[tokio::test]
    async fn deterministic_and_normalized() {
        let e = LocalEmbedder::new(128);
        let a = e
            .embed_query("retry with exponential backoff")
            .await
            .unwrap();
        let b = e
            .embed_query("retry with exponential backoff")
            .await
            .unwrap();
        assert_eq!(a, b, "same text ⇒ same vector");
        assert_eq!(a.len(), 128);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "L2-normalised, got {norm}");
    }

    #[tokio::test]
    async fn overlapping_text_is_more_similar_than_disjoint() {
        let e = LocalEmbedder::default();
        let base = e.embed_query("parse the json config file").await.unwrap();
        let near = e.embed_query("parse the json config").await.unwrap();
        let far = e.embed_query("render a png image").await.unwrap();
        assert!(
            cosine_similarity(&base, &near) > cosine_similarity(&base, &far),
            "shared tokens ⇒ higher cosine"
        );
    }

    #[tokio::test]
    async fn batch_matches_single() {
        let e = LocalEmbedder::new(64);
        let single = e.embed_query("hello world").await.unwrap();
        let batch = e.embed_docs(&["hello world".to_string()]).await.unwrap();
        assert_eq!(batch[0], single);
        assert_eq!(e.dimensions(), 64);
    }

    #[tokio::test]
    async fn dims_clamped_to_minimum() {
        assert_eq!(LocalEmbedder::new(1).dimensions(), 16);
    }
}
