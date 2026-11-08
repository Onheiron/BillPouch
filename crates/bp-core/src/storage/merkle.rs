use serde::{Deserialize, Serialize};

pub const BLOCK: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    pub index: usize,
    pub block: Vec<u8>,
    pub path: Vec<[u8; 32]>,
}

pub fn block_count(len: usize) -> usize {
    len.div_ceil(BLOCK).max(1)
}

fn leaf(index: usize, block: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("billpouch/merkle-leaf/v1");
    h.update(&(index as u64).to_le_bytes());
    h.update(block);
    *h.finalize().as_bytes()
}

fn node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("billpouch/merkle-node/v1");
    h.update(left);
    h.update(right);
    *h.finalize().as_bytes()
}

fn block(data: &[u8], index: usize) -> &[u8] {
    let start = (index * BLOCK).min(data.len());
    &data[start..(start + BLOCK).min(data.len())]
}

fn levels(data: &[u8]) -> Vec<Vec<[u8; 32]>> {
    let mut level: Vec<[u8; 32]> = (0..block_count(data.len()))
        .map(|i| leaf(i, block(data, i)))
        .collect();
    let mut all = vec![level.clone()];
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| node(&pair[0], pair.get(1).unwrap_or(&pair[0])))
            .collect();
        all.push(level.clone());
    }
    all
}

pub fn root(data: &[u8]) -> [u8; 32] {
    levels(data).last().expect("at least one level")[0]
}

pub fn prove(data: &[u8], index: usize) -> Option<Proof> {
    if index >= block_count(data.len()) {
        return None;
    }
    let all = levels(data);
    let mut path = Vec::new();
    let mut i = index;
    for level in &all[..all.len() - 1] {
        let sibling = if i.is_multiple_of(2) { i + 1 } else { i - 1 };
        path.push(*level.get(sibling).unwrap_or(&level[i]));
        i /= 2;
    }
    Some(Proof {
        index,
        block: block(data, index).to_vec(),
        path,
    })
}

pub fn verify(root: &[u8; 32], blocks: usize, proof: &Proof) -> bool {
    if proof.index >= blocks || proof.block.len() > BLOCK {
        return false;
    }
    let mut width = blocks;
    let mut depth = 0;
    while width > 1 {
        width = width.div_ceil(2);
        depth += 1;
    }
    if proof.path.len() != depth {
        return false;
    }
    let mut hash = leaf(proof.index, &proof.block);
    let mut i = proof.index;
    for sibling in &proof.path {
        hash = if i.is_multiple_of(2) {
            node(&hash, sibling)
        } else {
            node(sibling, &hash)
        };
        i /= 2;
    }
    hash == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn every_block_proves_against_the_root() {
        for len in [1, BLOCK, BLOCK + 1, 3 * BLOCK, 5 * BLOCK + 7] {
            let d = data(len);
            let r = root(&d);
            let blocks = block_count(len);
            for i in 0..blocks {
                let p = prove(&d, i).unwrap();
                assert!(verify(&r, blocks, &p), "len {len} block {i}");
            }
            assert!(prove(&d, blocks).is_none());
        }
    }

    #[test]
    fn a_changed_block_or_path_fails() {
        let d = data(5 * BLOCK);
        let r = root(&d);
        let mut p = prove(&d, 2).unwrap();
        p.block[0] ^= 1;
        assert!(!verify(&r, 5, &p));
        let mut p = prove(&d, 2).unwrap();
        p.path[0][0] ^= 1;
        assert!(!verify(&r, 5, &p));
        let mut p = prove(&d, 2).unwrap();
        p.index = 3;
        assert!(!verify(&r, 5, &p));
        let p = prove(&d, 2).unwrap();
        assert!(!verify(&r, 9, &p));
    }
}
