//! Cache of the quantized INT8 weights of a packed `.swz` model.
//!
//! Quantizing at load time leaves a private copy of the weights in every
//! process, and in every copy of the library a process has loaded. A cache
//! file holds the quantized matrices instead, and each process maps it, so
//! all of them share one copy in the page cache.
//!
//! The file is written to a temporary name and renamed into place, so a
//! reader never sees a partial file. It is never modified after that.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::kernels::Q8Matrix;

/// Bump the last byte when the quantization or the layout changes.
const MAGIC: &[u8; 8] = b"SWZQ8\0\0\x01";
const HEADER: usize = 24;
const ENTRY: usize = 32;
const SAMPLES: usize = 64;
const SAMPLE_LEN: usize = 4096;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// Identifies the model file: its length, modification time, header and
/// evenly spaced samples of its weights.
pub(super) fn key(file: &File, map: &[u8], header_len: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325;
    fnv(&mut h, &(map.len() as u64).to_le_bytes());
    if let Some(t) = file
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        fnv(&mut h, &t.as_nanos().to_le_bytes());
    }
    fnv(&mut h, &map[..header_len.min(map.len())]);
    let last = map.len().saturating_sub(SAMPLE_LEN);
    for i in 0..SAMPLES {
        let at = last * i / (SAMPLES - 1);
        fnv(&mut h, &map[at..(at + SAMPLE_LEN).min(map.len())]);
    }
    h
}

fn cache_name(model: &Path) -> String {
    let stem = model
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cache_path(dir: &Path, model: &Path, key: u64) -> PathBuf {
    dir.join(format!("{}-{key:016x}.q8", cache_name(model)))
}

/// The quantized matrices of `model` with the given shapes: mapped from the
/// cache in `dir` when possible, or built by `quantize` and cached.
pub(super) fn load_or_build(
    dir: &Path,
    model: &Path,
    key: u64,
    shapes: &[(usize, usize)],
    quantize: impl FnOnce() -> Vec<Q8Matrix>,
) -> Vec<Q8Matrix> {
    let path = cache_path(dir, model, key);
    if let Some(mats) = load(&path, key, shapes) {
        return mats;
    }
    let mats = quantize();
    if store(&path, key, &mats).is_ok() {
        remove_stale(dir, model, &path);
        if let Some(mapped) = load(&path, key, shapes) {
            return mapped;
        }
    }
    mats
}

fn read_u64(map: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(map.get(at..at + 8)?.try_into().ok()?))
}

fn load(path: &Path, key: u64, shapes: &[(usize, usize)]) -> Option<Vec<Q8Matrix>> {
    let file = File::open(path).ok()?;
    // SAFETY: cache files are only replaced by renaming, never modified.
    let map = Arc::new(unsafe { memmap2::Mmap::map(&file) }.ok()?);
    if map.get(..8)? != MAGIC
        || read_u64(&map, 8)? != key
        || read_u64(&map, 16)? != shapes.len() as u64
    {
        return None;
    }
    shapes
        .iter()
        .enumerate()
        .map(|(i, &(k, n))| {
            let at = HEADER + i * ENTRY;
            let field = |j: usize| read_u64(&map, at + j * 8).and_then(|v| usize::try_from(v).ok());
            if field(0)? != k || field(1)? != n {
                return None;
            }
            Q8Matrix::mapped(&map, k, n, field(2)?, field(3)?)
        })
        .collect()
}

fn align(n: usize) -> usize {
    n.div_ceil(64) * 64
}

fn store(path: &Path, key: u64, mats: &[Q8Matrix]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let result = write(&tmp, key, mats).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write(path: &Path, key: u64, mats: &[Q8Matrix]) -> io::Result<()> {
    let mut offsets = Vec::with_capacity(mats.len());
    let mut end = align(HEADER + mats.len() * ENTRY);
    for m in mats {
        let q = end;
        let scales = align(q + m.q_len());
        end = align(scales + m.scales_len() * 4);
        offsets.push((q, scales));
    }
    let mut w = Counted {
        w: BufWriter::new(File::create(path)?),
        pos: 0,
    };
    w.put(MAGIC)?;
    w.put(&key.to_le_bytes())?;
    w.put(&(mats.len() as u64).to_le_bytes())?;
    for (m, &(q, scales)) in mats.iter().zip(&offsets) {
        for v in [m.k(), m.n(), q, scales] {
            w.put(&(v as u64).to_le_bytes())?;
        }
    }
    for (m, &(q, scales)) in mats.iter().zip(&offsets) {
        w.pad_to(q)?;
        let qs = m.q();
        // SAFETY: i8 has the size and alignment of u8.
        w.put(unsafe { std::slice::from_raw_parts(qs.as_ptr() as *const u8, qs.len()) })?;
        w.pad_to(scales)?;
        for s in m.scales() {
            w.put(&s.to_le_bytes())?;
        }
    }
    w.w.flush()?;
    w.w.into_inner().map_err(|e| e.into_error())?.sync_all()
}

struct Counted {
    w: BufWriter<File>,
    pos: usize,
}

impl Counted {
    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.w.write_all(bytes)?;
        self.pos += bytes.len();
        Ok(())
    }

    fn pad_to(&mut self, at: usize) -> io::Result<()> {
        self.put(&[0u8; 64][..at - self.pos])
    }
}

/// Removes caches of earlier versions of `model`. Processes that still map
/// one keep reading it; the file goes away when the last one unmaps it.
fn remove_stale(dir: &Path, model: &Path, keep: &Path) {
    let prefix = format!("{}-", cache_name(model));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let hex = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".q8"));
        if path != keep
            && hex.is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrices() -> Vec<Q8Matrix> {
        let w: Vec<f32> = (0..40 * 70)
            .map(|i| ((i * 7 % 23) as f32 - 11.0) / 5.0)
            .collect();
        vec![
            Q8Matrix::from_f32(&w, 40, 70),
            Q8Matrix::from_f32(&w, 70, 40),
        ]
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sweet-zenz-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn mapped_matrices_match_the_quantized_ones() {
        let dir = temp_dir("q8cache");
        let model = Path::new("zenz model.swz");
        let shapes = [(40, 70), (70, 40)];
        let built = load_or_build(&dir, model, 7, &shapes, matrices);
        let cached = load_or_build(&dir, model, 7, &shapes, || panic!("the cache was not used"));
        let fresh = matrices();
        for m in [&built, &cached] {
            for (a, b) in m.iter().zip(&fresh) {
                assert_eq!(a.q(), b.q());
                assert_eq!(a.scales(), b.scales());
            }
        }
        assert!(cache_path(&dir, model, 7).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_model_replaces_the_old_cache() {
        let dir = temp_dir("q8stale");
        let model = Path::new("zenz.swz");
        let shapes = [(40, 70), (70, 40)];
        // Another process still maps the old cache.
        let old = load_or_build(&dir, model, 1, &shapes, matrices);
        load_or_build(&dir, model, 2, &shapes, matrices);
        assert_eq!(old[0].q(), matrices()[0].q());
        assert!(load(&cache_path(&dir, model, 2), 2, &shapes).is_some());
        assert!(load(&cache_path(&dir, model, 2), 1, &shapes).is_none());
        assert!(load(&cache_path(&dir, model, 2), 2, &[(40, 70)]).is_none());
        assert!(!cache_path(&dir, model, 1).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
