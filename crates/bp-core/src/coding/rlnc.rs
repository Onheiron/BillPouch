use crate::error::{BpError, BpResult};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::gf256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedFragment {
    pub id: String,
    pub chunk_id: String,
    pub k: usize,
    pub coding_vector: Vec<u8>,
    pub data: Vec<u8>,
}

impl EncodedFragment {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.k + self.data.len());
        out.extend_from_slice(b"BPFG");
        out.extend_from_slice(&(self.k as u32).to_le_bytes());
        out.extend_from_slice(&self.coding_vector);
        out.extend_from_slice(&self.data);
        out
    }

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

    for i in 0..n {
        let coeffs: Vec<u8> = if i < k {
            (0..k).map(|j| if j == i { 1u8 } else { 0u8 }).collect()
        } else {
            (0..k).map(|_| rng.gen::<u8>()).collect()
        };
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
        let (new_data, new_vector) = loop {
            let coeffs: Vec<u8> = (0..fragments.len()).map(|_| rng.gen::<u8>()).collect();

            let mut data = vec![0u8; sym_size];
            let mut vector = vec![0u8; k];
            for (frag, &a) in fragments.iter().zip(coeffs.iter()) {
                gf256::mul_acc(&mut data, &frag.data, a);
                gf256::mul_acc(&mut vector, &frag.coding_vector, a);
            }

            if vector.iter().any(|&b| b != 0) {
                break (data, vector);
            }
        };

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

    let mut matrix: Vec<Vec<u8>> = fragments[..k]
        .iter()
        .map(|f| {
            let mut row = f.coding_vector.clone();
            row.extend_from_slice(&f.data);
            row
        })
        .collect();

    for col in 0..k {
        let pivot = (col..k).find(|&r| matrix[r][col] != 0).ok_or_else(|| {
            BpError::Coding(format!(
                "decode: matrix is singular at column {col} (linearly dependent fragments)"
            ))
        })?;
        matrix.swap(col, pivot);

        let pivot_val = matrix[col][col];
        let pivot_inv = gf256::inv(pivot_val);
        for elem in matrix[col].iter_mut() {
            *elem = gf256::mul(*elem, pivot_inv);
        }

        for row in 0..k {
            if row == col {
                continue;
            }
            let factor = matrix[row][col];
            if factor == 0 {
                continue;
            }
            let pivot_row: Vec<u8> = matrix[col].clone();
            for (dst, &src) in matrix[row].iter_mut().zip(pivot_row.iter()) {
                *dst ^= gf256::mul(factor, src);
            }
        }
    }

    let mut result = Vec::with_capacity(k * sym_size);
    for row in &matrix {
        result.extend_from_slice(&row[k..]);
    }

    Ok(result)
}

pub fn chunk_hash(chunk: &[u8]) -> String {
    let hash = blake3::hash(chunk);
    hash.to_hex()[..16].to_string()
}

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

fn combine(symbols: &[Vec<u8>], coeffs: &[u8], sym_size: usize) -> Vec<u8> {
    let mut result = vec![0u8; sym_size];
    for (sym, &c) in symbols.iter().zip(coeffs.iter()) {
        gf256::mul_acc(&mut result, sym, c);
    }
    result
}

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
        let recovered = decode(&frags[..k]).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn decode_any_k_subset() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 4).unwrap();
        let reversed: Vec<_> = frags[..k].iter().cloned().rev().collect();
        let recovered = decode(&reversed).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn recode_produces_valid_fragments() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k + 4).unwrap();
        let recoded = recode(&frags[..2], 3).unwrap();
        assert_eq!(recoded.len(), 3);
        for f in &recoded {
            assert_eq!(f.coding_vector.len(), k);
            assert_eq!(f.data.len(), frags[0].data.len());
            assert!(
                f.coding_vector.iter().any(|&b| b != 0),
                "recode must never produce an all-zero coding vector"
            );
        }
    }

    #[test]
    fn decode_from_recoded_fragments() {
        let k = 4;
        let frags = encode(TEST_CHUNK, k, k).unwrap();
        let chunk_id = frags[0].chunk_id.clone();

        let recoded: Vec<EncodedFragment> = (0..k)
            .map(|i| {
                let j = (i + 2) % k;
                let coding_vector = (0..k).map(|c| if c == j { 1u8 } else { 0u8 }).collect();
                let data = frags[j].data.clone();
                EncodedFragment {
                    id: uuid::Uuid::new_v4().to_string(),
                    chunk_id: chunk_id.clone(),
                    k,
                    coding_vector,
                    data,
                }
            })
            .collect();

        let recovered = decode(&recoded).unwrap();
        assert!(recovered.starts_with(TEST_CHUNK));
    }

    #[test]
    fn decode_only_recoded_no_originals() {
        let k = 3;
        let frags = encode(TEST_CHUNK, k, k).unwrap();
        let chunk_id = frags[0].chunk_id.clone();

        let recoded: Vec<EncodedFragment> = (0..k)
            .map(|i| {
                let j = (i + 1) % k;
                let coding_vector = (0..k).map(|c| if c == j { 1u8 } else { 0u8 }).collect();
                let data = frags[j].data.clone();
                EncodedFragment {
                    id: uuid::Uuid::new_v4().to_string(),
                    chunk_id: chunk_id.clone(),
                    k,
                    coding_vector,
                    data,
                }
            })
            .collect();

        for (i, f) in recoded.iter().enumerate() {
            let identity_i: Vec<u8> = (0..k).map(|j| if j == i { 1u8 } else { 0u8 }).collect();
            assert_ne!(f.coding_vector, identity_i);
        }
        let recovered = decode(&recoded).unwrap();
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
