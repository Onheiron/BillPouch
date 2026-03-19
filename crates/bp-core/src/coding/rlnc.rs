//! Random Linear Network Coding (RLNC) over GF(2⁸).
//!
//! ## Encoding
//!
//! A chunk of bytes is split into `k` equally-sized **source symbols**.
//! For each of the `n` encoded fragments, `k` random coefficients are drawn
//! from GF(2⁸) and a linear combination is computed:
//!
//! ```text
//! fragment_data[i]    = Σ coeff[i][j] · source_symbol[j]   (over GF(2⁸), byte-wise)
//! coding_vector[i]    = [coeff[i][0], coeff[i][1], ..., coeff[i][k-1]]
//! ```
//!
//! ## Recoding — no decompression required
//!
//! A Pouch holding `m` fragments can produce new fragments by recombining them:
//!
//! ```text
//! new_data    = Σ a[i] · fragment_data[i]
//! new_vector  = Σ a[i] · coding_vector[i]
//! ```
//!
//! The result is a valid encoded fragment of the same chunk.
//! **The Pouch never touches the original chunk.**
//!
//! ## Decoding
//!
//! Any `k` linearly independent fragments suffice.  Gaussian elimination over
//! GF(2⁸) on the `[coding_vectors | fragment_data]` augmented matrix yields
//! the source symbols, which are then concatenated to recover the chunk.

use crate::error::{BpError, BpResult};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::gf256;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single RLNC-encoded fragment of a chunk.
///
/// Can be stored on disk, transmitted over the network, or used as input to
/// [`recode`] to generate additional fragments without decoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedFragment {
    /// Random UUID assigned at creation (encoding or recoding).
    pub id: String,
    /// BLAKE3 hash of the original chunk, hex-encoded (first 16 chars = 64 bits).
    /// Used as the key in the fragment store and for integrity verification.
    pub chunk_id: String,
    /// Number of source symbols `k` this fragment was derived from.
    pub k: usize,
    /// Coefficients over GF(2⁸): one per source symbol.
    /// Length == `k`.
    pub coding_vector: Vec<u8>,
    /// Linear combination of source symbols over GF(2⁸).
    /// Length == `symbol_size` (chunk_size / k, padded).
    pub data: Vec<u8>,
}

impl EncodedFragment {
    /// Serialize to the on-disk binary format.
    ///
    /// ```text
    /// [0..4]      magic: b"BPFG"
    /// [4..8]      k: u32 LE
    /// [8..8+k]    coding_vector
    /// [8+k..]     data
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.k + self.data.len());
        out.extend_from_slice(b"BPFG");
        out.extend_from_slice(&(self.k as u32).to_le_bytes());
        out.extend_from_slice(&self.coding_vector);
        out.extend_from_slice(&self.data);
        out
    }

    /// Deserialize from the on-disk binary format.
    ///
    /// `id` and `chunk_id` are not stored in the binary blob — they come from
    /// the filename / index and must be supplied by the caller.
    pub fn from_bytes(id: String, chunk_id: String, bytes: &[u8]) -> BpResult<Self> {
        if bytes.len() < 8 || &bytes[0..4] != b"BPFG" {
            return Err(BpError::Coding("Invalid fragment magic bytes".into()));
        }
        let k = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        if bytes.len() < 8 + k {
            return Err(BpError::Coding(
                "Fragment too short for coding vector".into(),
            ));
        }
        let coding_vector = bytes[8..8 + k].to_vec();
        let data = bytes[8 + k..].to_vec();
        Ok(Self {
            id,
            chunk_id,
            k,
            coding_vector,
            data,
        })
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Split `chunk` into `k` source symbols and produce `n` encoded fragments.
///
/// # Arguments
/// - `chunk`     — raw bytes of the chunk to encode.
/// - `k`         — number of source symbols (recovery threshold).
/// - `n`         — total number of encoded fragments to produce (n ≥ k).
///
/// # Errors
/// Returns an error if `n < k` or `chunk` is empty.
pub fn encode(chunk: &[u8], k: usize, n: usize) -> BpResult<Vec<EncodedFragment>> {
    if chunk.is_empty() {
        return Err(BpError::Coding("Cannot encode empty chunk".into()));
    }
    if n < k {
        return Err(BpError::Coding(format!("n ({n}) must be >= k ({k})")));
    }
    if k == 0 {
        return Err(BpError::Coding("k must be > 0".into()));
    }

    let chunk_id = chunk_hash(chunk);
    let (symbols, sym_size) = split_into_symbols(chunk, k);

    let mut rng = rand::thread_rng();
    let mut fragments = Vec::with_capacity(n);

    for _ in 0..n {
        let coeffs: Vec<u8> = (0..k).map(|_| rng.gen::<u8>()).collect();
        let data = combine(&symbols, &coeffs, sym_size);
        fragments.push(EncodedFragment {
            id: Uuid::new_v4().to_string(),
            chunk_id: chunk_id.clone(),
            k,
            coding_vector: coeffs,
            data,
        });
    }

    Ok(fragments)
}

/// Produce `count` new fragments by recombining existing ones.
///
/// **No decoding is performed.** The Pouch only needs the fragments it holds.
/// The resulting fragments are valid RLNC fragments of the same original chunk.
///
/// Requires at least 1 input fragment.
///
/// # Errors
/// Returns an error if `fragments` is empty or `count` is 0.
pub fn recode(fragments: &[EncodedFragment], count: usize) -> BpResult<Vec<EncodedFragment>> {
    if fragments.is_empty() {
        return Err(BpError::Coding("recode: no input fragments".into()));
    }
    if count == 0 {
        return Err(BpError::Coding("recode: count must be > 0".into()));
    }

    let k = fragments[0].k;
    let sym_size = fragments[0].data.len();
    let chunk_id = fragments[0].chunk_id.clone();

    let mut rng = rand::thread_rng();
    let mut result = Vec::with_capacity(count);

    for _ in 0..count {
        // Draw random recoding coefficients a[i] for each input fragment
        let coeffs: Vec<u8> = (0..fragments.len()).map(|_| rng.gen::<u8>()).collect();

        // new_data    = Σ a[i] · fragment[i].data
        let mut new_data = vec![0u8; sym_size];
        for (frag, &a) in fragments.iter().zip(coeffs.iter()) {
            gf256::mul_acc(&mut new_data, &frag.data, a);
        }

        // new_vector  = Σ a[i] · fragment[i].coding_vector
        let mut new_vector = vec![0u8; k];
        for (frag, &a) in fragments.iter().zip(coeffs.iter()) {
            gf256::mul_acc(&mut new_vector, &frag.coding_vector, a);
        }

        result.push(EncodedFragment {
            id: Uuid::new_v4().to_string(),
            chunk_id: chunk_id.clone(),
            k,
            coding_vector: new_vector,
            data: new_data,
        });
    }

    Ok(result)
}

/// Reconstruct the original chunk from `k` (or more) linearly independent fragments.
///
/// Uses Gaussian elimination over GF(2⁸) on the augmented matrix
/// `[coding_vectors | fragment_data]`.
///
/// The returned bytes include any zero-padding added during encoding — the caller
/// is responsible for stripping trailing padding to the original length (stored in
/// the file manifest, not in the fragment itself).
///
/// # Errors
/// - Fewer than `k` fragments provided.
/// - Fragments are linearly dependent (singular matrix).
/// - Inconsistent `k` or `data` length across fragments.
pub fn decode(fragments: &[EncodedFragment]) -> BpResult<Vec<u8>> {
    if fragments.is_empty() {
        return Err(BpError::Coding("decode: no fragments provided".into()));
    }

    let k = fragments[0].k;
    let sym_size = fragments[0].data.len();

    if fragments.len() < k {
        return Err(BpError::Coding(format!(
            "decode: need {k} fragments, got {}",
            fragments.len()
        )));
    }

    // Validate consistency
    for (i, f) in fragments[..k].iter().enumerate() {
        if f.k != k {
            return Err(BpError::Coding(format!(
                "decode: fragment {i} has k={}, expected {k}",
                f.k
            )));
        }
        if f.data.len() != sym_size {
            return Err(BpError::Coding(format!(
                "decode: fragment {i} data len={}, expected {sym_size}",
                f.data.len()
            )));
        }
        if f.coding_vector.len() != k {
            return Err(BpError::Coding(format!(
                "decode: fragment {i} coding_vector len={}, expected {k}",
                f.coding_vector.len()
            )));
        }
    }

    // Build augmented matrix: each row = [coding_vector (k bytes) | data (sym_size bytes)]
    let row_len = k + sym_size;
    let mut matrix: Vec<Vec<u8>> = fragments[..k]
        .iter()
        .map(|f| {
            let mut row = f.coding_vector.clone();
            row.extend_from_slice(&f.data);
            row
        })
        .collect();

    // Gaussian elimination over GF(2⁸)
    for col in 0..k {
        // Find a non-zero pivot in column `col` at or below the current row
        let pivot = (col..k).find(|&r| matrix[r][col] != 0).ok_or_else(|| {
            BpError::Coding(format!(
                "decode: matrix is singular at column {col} (linearly dependent fragments)"
            ))
        })?;
        matrix.swap(col, pivot);

        // Normalise pivot row so M[col][col] == 1
        let pivot_val = matrix[col][col];
        let pivot_inv = gf256::inv(pivot_val);
        for j in 0..row_len {
            matrix[col][j] = gf256::mul(matrix[col][j], pivot_inv);
        }

        // Eliminate column `col` from all other rows
        for row in 0..k {
            if row == col {
                continue;
            }
            let factor = matrix[row][col];
            if factor == 0 {
                continue;
            }
            // Borrow trick: copy the pivot row to avoid borrow conflict
            let pivot_row: Vec<u8> = matrix[col].clone();
            for j in 0..row_len {
                let v = gf256::mul(factor, pivot_row[j]);
                matrix[row][j] ^= v;
            }
        }
    }

    // After reduction to row echelon form, row i holds source symbol i
    // in the right half of the augmented matrix.
    let mut result = Vec::with_capacity(k * sym_size);
    for row in &matrix {
        result.extend_from_slice(&row[k..]);
    }

    Ok(result)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// BLAKE3 hash of a chunk, hex-encoded, first 16 chars (64-bit prefix).
pub fn chunk_hash(chunk: &[u8]) -> String {
    let hash = blake3::hash(chunk);
    hash.to_hex()[..16].to_string()
}

/// Split `chunk` into exactly `k` symbols of equal byte length, zero-padding
/// the last symbol if the chunk size is not divisible by `k`.
fn split_into_symbols(chunk: &[u8], k: usize) -> (Vec<Vec<u8>>, usize) {
    let sym_size = chunk.len().div_ceil(k);
    let mut symbols = Vec::with_capacity(k);
    for i in 0..k {
        let start = i * sym_size;
        let end = ((i + 1) * sym_size).min(chunk.len());
        let mut sym = vec![0u8; sym_size];
        if start < chunk.len() {
            sym[..end - start].copy_from_slice(&chunk[start..end]);
        }
        symbols.push(sym);
    }
    (symbols, sym_size)
}

/// Compute a linear combination of `symbols` with given `coeffs` over GF(2⁸).
fn combine(symbols: &[Vec<u8>], coeffs: &[u8], sym_size: usize) -> Vec<u8> {
    let mut result = vec![0u8; sym_size];
    for (sym, &c) in symbols.iter().zip(coeffs.iter()) {
        gf256::mul_acc(&mut result, sym, c);
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CHUNK: &[u8] = b"Hello BillPouch! This is a test chunk for RLNC encoding.";

    #[test]
    fn encode_produces_n_fragments() {
        let frags = encode(TEST_CHUNK, 4, 8).unwrap();
        assert_eq!(frags.len(), 8);
        for f in &frags {
            assert_eq!(f.k, 4);
            assert_eq!(f.coding_vector.len(), 4);
            assert!(!f.id.is_empty());
            assert_eq!(f.chunk_id.len(), 16);
        }
    }

    #[test]
    fn decode_exact_k_fragments() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 4).unwrap();
        // Use exactly the first k
        let recovered = decode(&frags[..k]).unwrap();
        // The recovered bytes may be padded — original must be a prefix
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn decode_any_k_subset() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 6).unwrap();
        // Try recovering from fragments [2..6] (skip first two)
        let recovered = decode(&frags[2..2 + k]).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn recode_produces_valid_fragments() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 4).unwrap();
        // Recode 3 new fragments from the first 2 existing ones
        let recoded = recode(&frags[..2], 3).unwrap();
        assert_eq!(recoded.len(), 3);
        for f in &recoded {
            assert_eq!(f.coding_vector.len(), k);
            assert_eq!(f.data.len(), frags[0].data.len());
        }
    }

    #[test]
    fn decode_from_recoded_fragments() {
        // A Pouch only has 2 fragments; it recodes to generate 2 more.
        // Together we have 4 = k, which should decode correctly.
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 2).unwrap();

        // Pouch A has fragment 0 and 1; recodes 2 new ones
        let recoded = recode(&frags[..2], 2).unwrap();

        // Combine: fragment 2, fragment 3 (original) + 2 recoded
        let mut to_decode: Vec<EncodedFragment> = frags[2..4].to_vec();
        to_decode.extend(recoded);

        let recovered = decode(&to_decode[..k]).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn decode_only_recoded_no_originals() {
        // Extreme case: k Pouches each have 1 fragment and recode independently.
        // Nobody has > 1 original fragment, yet decoding succeeds.
        let k = 3;
        let frags = encode(TEST_CHUNK, k, k + 3).unwrap();

        // Each "pouch" holds exactly 1 original fragment and recodes 1 new one
        let mut pool: Vec<EncodedFragment> = Vec::new();
        for frag in &frags[..k] {
            let recoded = recode(std::slice::from_ref(frag), 1).unwrap();
            pool.extend(recoded);
        }

        let recovered = decode(&pool[..k]).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn serialization_roundtrip() {
        let frags = encode(TEST_CHUNK, 3, 5).unwrap();
        let f = &frags[0];
        let bytes = f.to_bytes();
        let back = EncodedFragment::from_bytes(f.id.clone(), f.chunk_id.clone(), &bytes).unwrap();
        assert_eq!(back.k, f.k);
        assert_eq!(back.coding_vector, f.coding_vector);
        assert_eq!(back.data, f.data);
    }

    #[test]
    fn encode_error_on_empty_chunk() {
        assert!(encode(&[], 4, 8).is_err());
    }

    #[test]
    fn encode_error_n_less_than_k() {
        assert!(encode(TEST_CHUNK, 4, 2).is_err());
    }

    #[test]
    fn decode_error_too_few_fragments() {
        let frags = encode(TEST_CHUNK, 4, 6).unwrap();
        assert!(decode(&frags[..2]).is_err());
    }
}
