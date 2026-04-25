//! Deterministic in-process embedder (D27) and cosine / NaN-safe-ordering
//! helpers (D32, D1.M4 from the Round 1 review).
//!
//! The default `HashedShingleEmbedder` hashes token shingles (width 2) and
//! character n-grams (n = 3, 4, 5) into a fixed-size sparse vector and
//! L2-normalizes. No API calls, no hosted models. The SQLite BLOB column
//! is opaque to vector shape, so a future Spec 3 swap is schema-safe.

#![allow(dead_code)]

use std::cmp::Ordering;

use blake3::Hasher;

pub trait Embedder: Send + Sync {
    /// Embed a short text (goal + subgoal concat, ~5-30 tokens) into a
    /// fixed-shape vector. Shape is impl-defined; callers do not inspect.
    fn embed(&self, text: &str) -> Vec<f32>;
    fn impl_id(&self) -> &'static str;
}

pub struct HashedShingleEmbedder {
    pub dim: usize,
}

impl Default for HashedShingleEmbedder {
    fn default() -> Self {
        Self { dim: 4096 }
    }
}

impl Embedder for HashedShingleEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; self.dim];
        let lower = text.to_ascii_lowercase();

        // Token shingles width 2 (adjacent pairs)
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        for pair in tokens.windows(2) {
            bump_feature(&mut v, self.dim, &format!("t2:{} {}", pair[0], pair[1]));
        }
        for tok in &tokens {
            bump_feature(&mut v, self.dim, &format!("t1:{}", tok));
        }

        // Character n-grams n = 3, 4, 5 over the lowered string
        let chars: Vec<char> = lower.chars().collect();
        for n in [3usize, 4, 5] {
            if chars.len() >= n {
                for i in 0..=chars.len() - n {
                    let s: String = chars[i..i + n].iter().collect();
                    bump_feature(&mut v, self.dim, &format!("c{}:{}", n, s));
                }
            }
        }

        l2_normalize(&mut v);
        v
    }

    fn impl_id(&self) -> &'static str {
        "hashed_shingle_v1"
    }
}

fn bump_feature(vec: &mut [f32], dim: usize, feature: &str) {
    let mut h = Hasher::new();
    h.update(feature.as_bytes());
    let bytes = h.finalize();
    // Take first 8 bytes as index, next byte's LSB as sign.
    let idx_bytes: [u8; 8] = bytes.as_bytes()[..8].try_into().unwrap();
    let idx = (u64::from_le_bytes(idx_bytes) as usize) % dim;
    let sign = if bytes.as_bytes()[8] & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    vec[idx] += sign;
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity bounded [-1, 1]. Returns 0.0 if either vector has
/// zero magnitude — this is the "fail-soft" behavior required by D32.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let mag = na.sqrt() * nb.sqrt();
    if mag == 0.0 || !mag.is_finite() {
        0.0
    } else {
        let c = dot / mag;
        if c.is_finite() { c } else { 0.0 }
    }
}

/// Descending total ordering that treats NaN as the lowest value.
/// Use instead of `partial_cmp(...).unwrap()` when sorting scores.
pub fn nan_safe_desc(a: f32, b: f32) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater, // treat NaN as lowest -> sort last in desc
        (false, true) => Ordering::Less,
        (false, false) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_is_deterministic() {
        let e = HashedShingleEmbedder::default();
        let a = e.embed("sign in with google");
        let b = e.embed("sign in with google");
        assert_eq!(a, b);
    }

    #[test]
    fn embed_has_unit_norm_for_nonempty_input() {
        let e = HashedShingleEmbedder::default();
        let v = e.embed("login flow");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_input_produces_zero_vector() {
        let e = HashedShingleEmbedder::default();
        let v = e.embed("");
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn similar_strings_score_highly() {
        let e = HashedShingleEmbedder::default();
        let a = e.embed("sign in with google");
        let b = e.embed("signing in with google");
        let sim = cosine(&a, &b);
        assert!(sim > 0.5, "expected > 0.5, got {}", sim);
    }

    #[test]
    fn unrelated_strings_score_low() {
        let e = HashedShingleEmbedder::default();
        let a = e.embed("download the pdf file");
        let b = e.embed("quit the application");
        let sim = cosine(&a, &b);
        assert!(sim < 0.3, "expected < 0.3, got {}", sim);
    }

    #[test]
    fn cosine_on_mismatched_lengths_returns_zero() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn cosine_on_zero_vector_returns_zero_not_nan() {
        assert_eq!(cosine(&[0.0, 0.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn nan_safe_desc_sorts_nan_last() {
        let mut v: Vec<f32> = vec![0.3, f32::NAN, 0.9, 0.1];
        v.sort_by(|a, b| nan_safe_desc(*a, *b));
        assert_eq!(v[0], 0.9);
        assert_eq!(v[1], 0.3);
        assert_eq!(v[2], 0.1);
        assert!(v[3].is_nan());
    }

    #[test]
    fn impl_id_is_stable() {
        assert_eq!(
            HashedShingleEmbedder::default().impl_id(),
            "hashed_shingle_v1"
        );
    }
}
