//! Recall Fate's checkpoint memory: a whole copy at the start of each turn (a key), then for each
//! action only the 4 KiB pages that changed since the checkpoint before it (a delta), both LZ4
//! compressed. Which pages changed is told by a 64-bit hash of each page, kept for the newest
//! checkpoint, so no second copy of the console's memory is needed. Going back to a checkpoint
//! decompresses its turn's key and lays the deltas up to it over that, in order.
//!
//! (Keeping changed pages XORed with their old contents was tried on the Thor, 2026-11-16: no
//! smaller, as the pages an action changes are rewritten whole, frame and audio buffers mostly,
//! and it needs a plain copy of the memory. On the first map an action keeps about 4.6 MB, the
//! first of a turn about 9.5 MB, a key about 24 MB.)

/// the four memories a save state holds: FCRAM, VRAM, DSP RAM, AXI WRAM
pub const MEMORIES: usize = 4;
pub const PAGE: usize = 4096;

pub type Hashes = [Vec<u64>; MEMORIES];
pub type Memories = [Vec<u8>; MEMORIES];

pub enum Pages {
    /// each memory whole, compressed
    Key([Vec<u8>; MEMORIES]),
    /// (memory, page) for each page that changed, and those pages one after another, compressed
    Delta { pages: Vec<(u8, u32)>, data: Vec<u8> },
}

impl Pages {
    pub fn is_key(&self) -> bool {
        matches!(self, Pages::Key(_))
    }

    /// what it keeps in memory
    pub fn bytes(&self) -> usize {
        match self {
            Pages::Key(memories) => memories.iter().map(Vec::len).sum(),
            Pages::Delta { pages, data } => pages.len() * 8 + data.len(),
        }
    }

    /// pages that changed in each memory (a key: none)
    pub fn changed(&self) -> [usize; MEMORIES] {
        let mut out = [0; MEMORIES];
        if let Pages::Delta { pages, .. } = self {
            for &(m, _) in pages {
                out[m as usize] += 1;
            }
        }
        out
    }
}

/// a page's hash (word-wise multiply-rotate; a collision would keep a stale page, at odds of about
/// one in 2^64 per page)
fn hash(page: &[u8]) -> u64 {
    let mut h = 0x9E37_79B9_7F4A_7C15u64 ^ page.len() as u64;
    let mut words = page.chunks_exact(8);
    for word in &mut words {
        h = (h.rotate_left(23) ^ u64::from_le_bytes(word.try_into().unwrap())).wrapping_mul(0xA076_1D64_78BD_642F);
    }
    for &b in words.remainder() {
        h = (h.rotate_left(23) ^ b as u64).wrapping_mul(0xA076_1D64_78BD_642F);
    }
    h ^ (h >> 29)
}

pub fn hashes(memory: &[u8]) -> Vec<u64> {
    memory.chunks(PAGE).map(hash).collect()
}

/// a key of the memories, and their hashes
pub fn key(memories: [&[u8]; MEMORIES]) -> (Pages, Hashes) {
    (Pages::Key(memories.map(lz4_flex::compress_prepend_size)), memories.map(hashes))
}

/// the pages that changed since `last` (the newest checkpoint's hashes, which become these
/// memories' own)
pub fn delta(memories: [&[u8]; MEMORIES], last: &mut Hashes) -> Pages {
    let mut pages = Vec::new();
    let mut raw = Vec::new();
    for (m, memory) in memories.iter().enumerate() {
        if last[m].len() != memory.len().div_ceil(PAGE) {
            last[m] = vec![!0; memory.len().div_ceil(PAGE)];
        }
        for (p, page) in memory.chunks(PAGE).enumerate() {
            let h = hash(page);
            if last[m][p] != h {
                pages.push((m as u8, p as u32));
                raw.extend_from_slice(page);
                last[m][p] = h;
            }
        }
    }
    Pages::Delta { pages, data: lz4_flex::compress_prepend_size(&raw) }
}

/// the memories at the end of a chain: a key, then the deltas after it, oldest first
pub fn rebuild(chain: &[&Pages]) -> Result<Memories, String> {
    let Some(Pages::Key(key)) = chain.first() else { return Err("the checkpoint's turn has no key".into()) };
    let mut out: Memories = Default::default();
    for (m, compressed) in key.iter().enumerate() {
        out[m] = lz4_flex::decompress_size_prepended(compressed).map_err(|e| format!("a checkpoint is damaged, {e}"))?;
    }
    for pages in &chain[1..] {
        let Pages::Delta { pages, data } = pages else { return Err("a key inside a turn's chain".into()) };
        let raw = lz4_flex::decompress_size_prepended(data).map_err(|e| format!("a checkpoint is damaged, {e}"))?;
        let mut at = 0;
        for &(m, p) in pages {
            let memory = out.get_mut(m as usize).ok_or("a checkpoint names a memory that is not there")?;
            let start = p as usize * PAGE;
            let end = (start + PAGE).min(memory.len());
            let len = end.checked_sub(start).ok_or("a checkpoint names a page that is not there")?;
            memory[start..end].copy_from_slice(raw.get(at..at + len).ok_or("a checkpoint is short")?);
            at += len;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(m: &Memories) -> [&[u8]; MEMORIES] {
        [&m[0], &m[1], &m[2], &m[3]]
    }

    #[test]
    fn a_chain_of_deltas_rebuilds_each_checkpoint() {
        let mut now: Memories = [vec![0u8; 10 * PAGE], vec![7u8; 3 * PAGE + 100], vec![1u8; PAGE], vec![2u8; 50]];
        let (k, mut last) = key(refs(&now));
        let first = now.clone();
        now[0][5 * PAGE + 3] = 9;
        now[1][3 * PAGE + 99] = 4; // the short last page
        let second = now.clone();
        let d1 = delta(refs(&now), &mut last);
        assert_eq!(d1.changed(), [1, 1, 0, 0]);
        now[0][5 * PAGE + 3] = 0; // back as it was in the key
        now[3][10] = 8;
        let d2 = delta(refs(&now), &mut last);
        assert_eq!(d2.changed(), [1, 0, 0, 1]);
        let d3 = delta(refs(&now), &mut last);
        assert_eq!(d3.changed(), [0; MEMORIES]);
        assert_eq!(rebuild(&[&k]).unwrap(), first);
        assert_eq!(rebuild(&[&k, &d1]).unwrap(), second);
        assert_eq!(rebuild(&[&k, &d1, &d2, &d3]).unwrap(), now);
        assert!(rebuild(&[&d1]).is_err());
        assert!(k.bytes() < now.iter().map(Vec::len).sum::<usize>());
    }
}
