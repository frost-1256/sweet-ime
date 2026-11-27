//! GGUF v2/v3 loader for GPT-2 (`zenz`) models, supporting F32, F16, BF16,
//! Q8_0, Q4_0, Q4_1, Q5_0, Q5_1, Q4_K, Q5_K, and Q6_K tensors.
//!
//! Only little-endian GGUF files (the format written by llama.cpp on all
//! common platforms) are supported; big-endian files are rejected with an
//! explicit error.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use super::weights::{f16_to_f32, invalid, layer_from};
use super::{Config, Model, R, Store};

const GGUF_MAGIC: &[u8; 4] = b"GGUF";
/// Maximum nesting depth accepted for metadata arrays.
const MAX_META_DEPTH: u32 = 8;
/// GGML supports at most four tensor dimensions.
const MAX_TENSOR_DIMS: usize = 4;
/// Minimum encoded size of a GGUF string (its u64 length prefix).
const MIN_STRING_BYTES: usize = 8;
/// Minimum encoded size of a tensor info record: name, n_dims, type, offset.
const MIN_TENSOR_INFO_BYTES: usize = 8 + 4 + 4 + 8;

// GGML tensor element / quantization types.
const GGML_TYPE_F32: u32 = 0;
const GGML_TYPE_F16: u32 = 1;
const GGML_TYPE_Q4_0: u32 = 2;
const GGML_TYPE_Q4_1: u32 = 3;
const GGML_TYPE_Q5_0: u32 = 6;
const GGML_TYPE_Q5_1: u32 = 7;
const GGML_TYPE_Q8_0: u32 = 8;
const GGML_TYPE_Q4_K: u32 = 12;
const GGML_TYPE_Q5_K: u32 = 13;
const GGML_TYPE_Q6_K: u32 = 14;
const GGML_TYPE_BF16: u32 = 30;

// GGUF metadata value types.
const META_U8: u32 = 0;
const META_I8: u32 = 1;
const META_U16: u32 = 2;
const META_I16: u32 = 3;
const META_U32: u32 = 4;
const META_I32: u32 = 5;
const META_F32: u32 = 6;
const META_BOOL: u32 = 7;
const META_STRING: u32 = 8;
const META_ARRAY: u32 = 9;
const META_U64: u32 = 10;
const META_I64: u32 = 11;
const META_F64: u32 = 12;

enum MetaValue {
    Uint(u64),
    Float(f64),
    String(String),
    StringArray(Vec<String>),
    Other,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| invalid("gguf offset overflow"))?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| invalid("truncated gguf file"))?;
        self.pos = end;
        Ok(slice)
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// Rejects `count` entries that cannot fit in the remaining bytes, then
    /// reserves capacity fallibly.
    fn check_count(&self, count: usize, min_entry_bytes: usize) -> io::Result<()> {
        if count > self.remaining() / min_entry_bytes.max(1) {
            return Err(invalid("gguf declared count exceeds file size"));
        }
        Ok(())
    }

    fn reserve_vec<T>(&self, count: usize, min_entry_bytes: usize) -> io::Result<Vec<T>> {
        self.check_count(count, min_entry_bytes)?;
        let mut v = Vec::new();
        v.try_reserve_exact(count)
            .map_err(|_| invalid("gguf declared count too large"))?;
        Ok(v)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn string(&mut self) -> io::Result<String> {
        let len = usize::try_from(self.u64()?).map_err(|_| invalid("gguf string too long"))?;
        let raw = self.take(len)?;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| invalid("invalid utf-8 in gguf"))
    }

    fn skip_value(&mut self, vtype: u32, depth: u32) -> io::Result<()> {
        if depth > MAX_META_DEPTH {
            return Err(invalid("gguf metadata arrays nested too deeply"));
        }
        match vtype {
            META_U8 | META_I8 | META_BOOL => {
                self.take(1)?;
            }
            META_U16 | META_I16 => {
                self.take(2)?;
            }
            META_U32 | META_I32 | META_F32 => {
                self.take(4)?;
            }
            META_U64 | META_I64 | META_F64 => {
                self.take(8)?;
            }
            META_STRING => {
                let len =
                    usize::try_from(self.u64()?).map_err(|_| invalid("gguf string too long"))?;
                self.take(len)?;
            }
            META_ARRAY => {
                let elem_type = self.u32()?;
                let count =
                    usize::try_from(self.u64()?).map_err(|_| invalid("gguf array too long"))?;
                for _ in 0..count {
                    self.skip_value(elem_type, depth + 1)?;
                }
            }
            _ => return Err(invalid(format!("unsupported gguf metadata type {vtype}"))),
        }
        Ok(())
    }

    fn read_value(&mut self, vtype: u32, keep_strings: bool) -> io::Result<MetaValue> {
        match vtype {
            META_U8 => Ok(MetaValue::Uint(u64::from(self.u8()?))),
            META_U16 => Ok(MetaValue::Uint(u64::from(self.u16()?))),
            META_U32 => Ok(MetaValue::Uint(u64::from(self.u32()?))),
            META_U64 => Ok(MetaValue::Uint(self.u64()?)),
            META_I8 | META_I16 | META_I32 | META_I64 | META_BOOL => {
                self.skip_value(vtype, 0)?;
                Ok(MetaValue::Other)
            }
            META_F32 => Ok(MetaValue::Float(f64::from(self.f32()?))),
            META_F64 => Ok(MetaValue::Float(self.f64()?)),
            META_STRING => Ok(MetaValue::String(self.string()?)),
            META_ARRAY => {
                let elem_type = self.u32()?;
                let count =
                    usize::try_from(self.u64()?).map_err(|_| invalid("gguf array too long"))?;
                if keep_strings && elem_type == META_STRING {
                    let mut out = self.reserve_vec(count, MIN_STRING_BYTES)?;
                    for _ in 0..count {
                        out.push(self.string()?);
                    }
                    Ok(MetaValue::StringArray(out))
                } else {
                    for _ in 0..count {
                        self.skip_value(elem_type, 1)?;
                    }
                    Ok(MetaValue::Other)
                }
            }
            _ => Err(invalid(format!("unsupported gguf metadata type {vtype}"))),
        }
    }
}

struct TensorInfo {
    dims: Vec<usize>,
    ggml_type: u32,
    offset: usize,
}

fn ggml_type_layout(ggml_type: u32) -> io::Result<(usize, usize)> {
    // Returns (block_elements, block_bytes).
    match ggml_type {
        GGML_TYPE_F32 => Ok((1, 4)),
        GGML_TYPE_F16 | GGML_TYPE_BF16 => Ok((1, 2)),
        GGML_TYPE_Q4_0 => Ok((32, 18)),
        GGML_TYPE_Q4_1 => Ok((32, 20)),
        GGML_TYPE_Q5_0 => Ok((32, 22)),
        GGML_TYPE_Q5_1 => Ok((32, 24)),
        GGML_TYPE_Q8_0 => Ok((32, 34)),
        GGML_TYPE_Q4_K => Ok((256, 144)),
        GGML_TYPE_Q5_K => Ok((256, 176)),
        GGML_TYPE_Q6_K => Ok((256, 210)),
        _ => Err(invalid(format!(
            "unsupported gguf quantization type {ggml_type}"
        ))),
    }
}

fn read_f16(b: &[u8]) -> f32 {
    f16_to_f32(u16::from_le_bytes([b[0], b[1]]))
}

fn read_bf16(b: &[u8]) -> f32 {
    f32::from_bits(u32::from(u16::from_le_bytes([b[0], b[1]])) << 16)
}

fn get_scale_min_k4(j: usize, q: &[u8]) -> (u8, u8) {
    if j < 4 {
        (q[j] & 63, q[j + 4] & 63)
    } else {
        (
            (q[j + 4] & 0x0f) | ((q[j - 4] >> 6) << 4),
            (q[j + 4] >> 4) | ((q[j] >> 6) << 4),
        )
    }
}

fn dequantize(raw: &[u8], ggml_type: u32, n_elems: usize) -> io::Result<Vec<f32>> {
    let (block_elems, block_bytes) = ggml_type_layout(ggml_type)?;
    if n_elems % block_elems != 0 {
        return Err(invalid(format!(
            "tensor element count {n_elems} not divisible by block size {block_elems}"
        )));
    }
    let n_blocks = n_elems / block_elems;
    let need = n_blocks
        .checked_mul(block_bytes)
        .ok_or_else(|| invalid("gguf tensor byte size overflow"))?;
    if raw.len() < need {
        return Err(invalid("truncated gguf tensor data"));
    }
    let raw = &raw[..need];
    let mut out = vec![0.0f32; n_elems];
    match ggml_type {
        GGML_TYPE_F32 => {
            for (dst, chunk) in out.iter_mut().zip(raw.chunks_exact(4)) {
                *dst = f32::from_le_bytes(chunk.try_into().unwrap());
            }
        }
        GGML_TYPE_F16 => {
            for (dst, chunk) in out.iter_mut().zip(raw.chunks_exact(2)) {
                *dst = read_f16(chunk);
            }
        }
        GGML_TYPE_BF16 => {
            for (dst, chunk) in out.iter_mut().zip(raw.chunks_exact(2)) {
                *dst = read_bf16(chunk);
            }
        }
        GGML_TYPE_Q8_0 => {
            for (b, block) in raw.chunks_exact(34).enumerate() {
                let d = read_f16(&block[0..2]);
                let dst = &mut out[b * 32..(b + 1) * 32];
                for (i, &q) in block[2..34].iter().enumerate() {
                    dst[i] = (q as i8) as f32 * d;
                }
            }
        }
        GGML_TYPE_Q4_0 => {
            for (b, block) in raw.chunks_exact(18).enumerate() {
                let d = read_f16(&block[0..2]);
                let dst = &mut out[b * 32..(b + 1) * 32];
                for (j, &byte) in block[2..18].iter().enumerate() {
                    let lo = (byte & 0x0f) as i32 - 8;
                    let hi = (byte >> 4) as i32 - 8;
                    dst[j] = lo as f32 * d;
                    dst[j + 16] = hi as f32 * d;
                }
            }
        }
        GGML_TYPE_Q4_1 => {
            for (b, block) in raw.chunks_exact(20).enumerate() {
                let d = read_f16(&block[0..2]);
                let m = read_f16(&block[2..4]);
                let dst = &mut out[b * 32..(b + 1) * 32];
                for (j, &byte) in block[4..20].iter().enumerate() {
                    let lo = (byte & 0x0f) as f32;
                    let hi = (byte >> 4) as f32;
                    dst[j] = lo * d + m;
                    dst[j + 16] = hi * d + m;
                }
            }
        }
        GGML_TYPE_Q5_0 => {
            for (b, block) in raw.chunks_exact(22).enumerate() {
                let d = read_f16(&block[0..2]);
                let qh = u32::from_le_bytes(block[2..6].try_into().unwrap());
                let dst = &mut out[b * 32..(b + 1) * 32];
                for (j, &byte) in block[6..22].iter().enumerate() {
                    let h0 = ((qh >> j) & 1) << 4;
                    let h1 = ((qh >> (j + 16)) & 1) << 4;
                    let lo = ((byte & 0x0f) as u32 | h0) as i32 - 16;
                    let hi = ((byte >> 4) as u32 | h1) as i32 - 16;
                    dst[j] = lo as f32 * d;
                    dst[j + 16] = hi as f32 * d;
                }
            }
        }
        GGML_TYPE_Q5_1 => {
            for (b, block) in raw.chunks_exact(24).enumerate() {
                let d = read_f16(&block[0..2]);
                let m = read_f16(&block[2..4]);
                let qh = u32::from_le_bytes(block[4..8].try_into().unwrap());
                let dst = &mut out[b * 32..(b + 1) * 32];
                for (j, &byte) in block[8..24].iter().enumerate() {
                    let h0 = ((qh >> j) & 1) << 4;
                    let h1 = ((qh >> (j + 16)) & 1) << 4;
                    let lo = ((byte & 0x0f) as u32 | h0) as f32;
                    let hi = ((byte >> 4) as u32 | h1) as f32;
                    dst[j] = lo * d + m;
                    dst[j + 16] = hi * d + m;
                }
            }
        }
        GGML_TYPE_Q4_K => {
            for (b, block) in raw.chunks_exact(144).enumerate() {
                let d = read_f16(&block[0..2]);
                let dmin = read_f16(&block[2..4]);
                let scales = &block[4..16];
                let qs = &block[16..144];
                let dst = &mut out[b * 256..(b + 1) * 256];
                let mut is = 0;
                let mut y = 0;
                for chunk in qs.chunks_exact(32) {
                    let (sc1, m1) = get_scale_min_k4(is, scales);
                    let d1 = d * sc1 as f32;
                    let min1 = dmin * m1 as f32;
                    let (sc2, m2) = get_scale_min_k4(is + 1, scales);
                    let d2 = d * sc2 as f32;
                    let min2 = dmin * m2 as f32;
                    for (l, &q) in chunk.iter().enumerate() {
                        dst[y + l] = d1 * (q & 0x0f) as f32 - min1;
                    }
                    for (l, &q) in chunk.iter().enumerate() {
                        dst[y + 32 + l] = d2 * (q >> 4) as f32 - min2;
                    }
                    y += 64;
                    is += 2;
                }
            }
        }
        GGML_TYPE_Q5_K => {
            for (b, block) in raw.chunks_exact(176).enumerate() {
                let d = read_f16(&block[0..2]);
                let dmin = read_f16(&block[2..4]);
                let scales = &block[4..16];
                let qh = &block[16..48];
                let qs = &block[48..176];
                let dst = &mut out[b * 256..(b + 1) * 256];
                let mut is = 0;
                let mut y = 0;
                let mut u1: u8 = 1;
                let mut u2: u8 = 2;
                for chunk in qs.chunks_exact(32) {
                    let (sc1, m1) = get_scale_min_k4(is, scales);
                    let d1 = d * sc1 as f32;
                    let min1 = dmin * m1 as f32;
                    let (sc2, m2) = get_scale_min_k4(is + 1, scales);
                    let d2 = d * sc2 as f32;
                    let min2 = dmin * m2 as f32;
                    for l in 0..32 {
                        let h1 = if (qh[l] & u1) != 0 { 16 } else { 0 };
                        dst[y + l] = d1 * ((chunk[l] & 0x0f) + h1) as f32 - min1;
                    }
                    for l in 0..32 {
                        let h2 = if (qh[l] & u2) != 0 { 16 } else { 0 };
                        dst[y + 32 + l] = d2 * ((chunk[l] >> 4) + h2) as f32 - min2;
                    }
                    y += 64;
                    is += 2;
                    u1 <<= 2;
                    u2 <<= 2;
                }
            }
        }
        GGML_TYPE_Q6_K => {
            for (b, block) in raw.chunks_exact(210).enumerate() {
                let ql = &block[0..128];
                let qh = &block[128..192];
                let scales = &block[192..208];
                let d = read_f16(&block[208..210]);
                let dst = &mut out[b * 256..(b + 1) * 256];
                for n in 0..2 {
                    let ql = &ql[n * 64..(n + 1) * 64];
                    let qh = &qh[n * 32..(n + 1) * 32];
                    let sc = &scales[n * 8..(n + 1) * 8];
                    let y = &mut dst[n * 128..(n + 1) * 128];
                    for l in 0..32 {
                        let is = l / 16;
                        let q1 = ((ql[l] & 0x0f) | (((qh[l] >> 0) & 3) << 4)) as i8 - 32;
                        let q2 = ((ql[l + 32] & 0x0f) | (((qh[l] >> 2) & 3) << 4)) as i8 - 32;
                        let q3 = ((ql[l] >> 4) | (((qh[l] >> 4) & 3) << 4)) as i8 - 32;
                        let q4 = ((ql[l + 32] >> 4) | (((qh[l] >> 6) & 3) << 4)) as i8 - 32;
                        y[l] = d * (sc[is] as i8) as f32 * q1 as f32;
                        y[l + 32] = d * (sc[is + 2] as i8) as f32 * q2 as f32;
                        y[l + 64] = d * (sc[is + 4] as i8) as f32 * q3 as f32;
                        y[l + 96] = d * (sc[is + 6] as i8) as f32 * q4 as f32;
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(out)
}

fn transpose_2d(src: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut dst = vec![0.0f32; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            dst[c * rows + r] = src[r * cols + c];
        }
    }
    dst
}

impl Model {
    /// Loads a little-endian GPT-2 (`zenz`) GGUF v2/v3 file, returning the
    /// model and its `(vocab.json, merges.txt)` tokenizer strings for packing
    /// into `.swz`. Big-endian GGUF files are rejected.
    pub fn load_gguf(path: impl AsRef<Path>) -> io::Result<(Self, String, String)> {
        // Read an owned snapshot so concurrent writers cannot change the
        // bytes while they are parsed.
        let bytes = std::fs::read(path.as_ref())?;
        Self::load_gguf_bytes(&bytes)
    }

    fn load_gguf_bytes(bytes: &[u8]) -> io::Result<(Self, String, String)> {
        let mut cur = Cursor::new(bytes);
        if cur.take(4)? != GGUF_MAGIC {
            return Err(invalid("not a GGUF file"));
        }
        let version = cur.u32()?;
        if version != 2 && version != 3 {
            if matches!(version.swap_bytes(), 2 | 3) {
                return Err(invalid(
                    "big-endian GGUF files are not supported (use a little-endian GGUF)",
                ));
            }
            return Err(invalid(format!("unsupported GGUF version {version}")));
        }
        let tensor_count =
            usize::try_from(cur.u64()?).map_err(|_| invalid("gguf tensor count overflow"))?;
        let kv_count =
            usize::try_from(cur.u64()?).map_err(|_| invalid("gguf metadata count overflow"))?;

        let mut arch = String::new();
        let mut alignment: usize = 32;
        let mut n_embd = None;
        let mut n_head = None;
        let mut n_layer = None;
        let mut n_positions = None;
        let mut eps = 1e-5f32;
        let mut tokens: Option<Vec<String>> = None;
        let mut merges: Option<Vec<String>> = None;

        for _ in 0..kv_count {
            let key = cur.string()?;
            let vtype = cur.u32()?;
            let keep_strings = matches!(
                key.as_str(),
                "tokenizer.ggml.tokens" | "tokenizer.ggml.merges"
            );
            let val = cur.read_value(vtype, keep_strings)?;
            match (key.as_str(), val) {
                ("general.architecture", MetaValue::String(s)) => arch = s,
                ("general.alignment", MetaValue::Uint(v)) => {
                    if v == 0 || v % 8 != 0 {
                        return Err(invalid(format!(
                            "invalid gguf alignment {v} (must be a positive multiple of 8)"
                        )));
                    }
                    alignment =
                        usize::try_from(v).map_err(|_| invalid("gguf alignment overflow"))?;
                }
                ("general.alignment", _) => return Err(invalid("invalid gguf alignment type")),
                ("gpt2.embedding_length", MetaValue::Uint(v)) => n_embd = Some(v as usize),
                ("gpt2.attention.head_count", MetaValue::Uint(v)) => n_head = Some(v as usize),
                ("gpt2.block_count", MetaValue::Uint(v)) => n_layer = Some(v as usize),
                ("gpt2.context_length", MetaValue::Uint(v)) => n_positions = Some(v as usize),
                ("gpt2.attention.layer_norm_epsilon", MetaValue::Float(v)) => eps = v as f32,
                ("tokenizer.ggml.tokens", MetaValue::StringArray(arr)) => tokens = Some(arr),
                ("tokenizer.ggml.merges", MetaValue::StringArray(arr)) => merges = Some(arr),
                _ => {}
            }
        }

        if arch != "gpt2" {
            return Err(invalid(format!(
                "unsupported GGUF architecture '{arch}' (only GPT-2 / zenz models are supported)"
            )));
        }
        let tokens = tokens.ok_or_else(|| invalid("gguf missing tokenizer.ggml.tokens"))?;
        let merges = merges.ok_or_else(|| invalid("gguf missing tokenizer.ggml.merges"))?;
        let cfg = Config {
            n_embd: n_embd.ok_or_else(|| invalid("gguf missing gpt2.embedding_length"))?,
            n_head: n_head.ok_or_else(|| invalid("gguf missing gpt2.attention.head_count"))?,
            n_layer: n_layer.ok_or_else(|| invalid("gguf missing gpt2.block_count"))?,
            n_positions: n_positions.ok_or_else(|| invalid("gguf missing gpt2.context_length"))?,
            vocab_size: tokens.len(),
            eps,
        };
        if cfg.n_embd == 0 || cfg.n_layer == 0 || cfg.n_positions == 0 || cfg.vocab_size == 0 {
            return Err(invalid("gguf model dimensions must be positive"));
        }
        if cfg.n_head == 0 || cfg.n_embd % cfg.n_head != 0 {
            return Err(invalid(format!(
                "gguf head count {} must be positive and divide embedding length {}",
                cfg.n_head, cfg.n_embd
            )));
        }

        cur.check_count(tensor_count, MIN_TENSOR_INFO_BYTES)?;
        let mut infos = HashMap::new();
        infos
            .try_reserve(tensor_count)
            .map_err(|_| invalid("gguf tensor count too large"))?;
        for _ in 0..tensor_count {
            let name = cur.string()?;
            let n_dims = cur.u32()? as usize;
            if n_dims > MAX_TENSOR_DIMS {
                return Err(invalid(format!(
                    "gguf tensor {name} has too many dimensions"
                )));
            }
            let mut dims = Vec::with_capacity(n_dims);
            for _ in 0..n_dims {
                dims.push(usize::try_from(cur.u64()?).map_err(|_| invalid("gguf dim overflow"))?);
            }
            let ggml_type = cur.u32()?;
            let offset =
                usize::try_from(cur.u64()?).map_err(|_| invalid("gguf tensor offset overflow"))?;
            infos.insert(
                name,
                TensorInfo {
                    dims,
                    ggml_type,
                    offset,
                },
            );
        }

        let data_start = cur
            .pos
            .div_ceil(alignment)
            .checked_mul(alignment)
            .ok_or_else(|| invalid("gguf tensor section offset overflow"))?;
        if data_start > bytes.len() {
            return Err(invalid("truncated gguf tensor section"));
        }
        let tensor_data = &bytes[data_start..];

        let mut flat: Vec<f32> = Vec::new();
        let mut push_aligned = |vals: &[f32]| -> R {
            let start = flat.len();
            flat.extend_from_slice(vals);
            flat.resize(flat.len().div_ceil(16) * 16, 0.0);
            start..start + vals.len()
        };

        // `expected` lists GGUF dims in order `[ne0, ne1, ...]` (ne0 innermost).
        let read_tensor =
            |name: &str, expected: &[usize], transpose: bool| -> io::Result<Vec<f32>> {
                let info = infos
                    .get(name)
                    .ok_or_else(|| invalid(format!("missing gguf tensor {name}")))?;
                if info.dims != expected {
                    return Err(invalid(format!(
                        "gguf tensor {name} has shape {:?}, expected {expected:?}",
                        info.dims
                    )));
                }
                let n_elems = info.dims.iter().try_fold(1usize, |acc, &d| {
                    acc.checked_mul(d)
                        .ok_or_else(|| invalid("gguf tensor shape overflow"))
                })?;
                let raw = tensor_data
                    .get(info.offset..)
                    .ok_or_else(|| invalid(format!("gguf tensor {name} offset out of range")))?;
                let vals = dequantize(raw, info.ggml_type, n_elems)?;
                if transpose {
                    if info.dims.len() != 2 {
                        return Err(invalid(format!("expected 2D gguf tensor {name}")));
                    }
                    // GGUF dims are [ne0, ne1] where ne0 is the contiguous inner
                    // dimension, so memory layout is row-major [ne1, ne0].
                    Ok(transpose_2d(&vals, info.dims[1], info.dims[0]))
                } else {
                    Ok(vals)
                }
            };

        let (v, e) = (cfg.vocab_size, cfg.n_embd);
        let wte_vals = read_tensor("token_embd.weight", &[e, v], false)?;
        let wpe_vals = read_tensor("position_embd.weight", &[e, cfg.n_positions], false)?;
        let wte = push_aligned(&wte_vals);
        let wpe = push_aligned(&wpe_vals);

        let mut layers = Vec::with_capacity(cfg.n_layer);
        for i in 0..cfg.n_layer {
            let specs: [(String, Vec<usize>, bool); 12] = [
                (format!("blk.{i}.attn_norm.weight"), vec![e], false),
                (format!("blk.{i}.attn_norm.bias"), vec![e], false),
                (format!("blk.{i}.attn_qkv.weight"), vec![e, 3 * e], true),
                (format!("blk.{i}.attn_qkv.bias"), vec![3 * e], false),
                (format!("blk.{i}.attn_output.weight"), vec![e, e], true),
                (format!("blk.{i}.attn_output.bias"), vec![e], false),
                (format!("blk.{i}.ffn_norm.weight"), vec![e], false),
                (format!("blk.{i}.ffn_norm.bias"), vec![e], false),
                (format!("blk.{i}.ffn_up.weight"), vec![e, 4 * e], true),
                (format!("blk.{i}.ffn_up.bias"), vec![4 * e], false),
                (format!("blk.{i}.ffn_down.weight"), vec![4 * e, e], true),
                (format!("blk.{i}.ffn_down.bias"), vec![e], false),
            ];
            let mut ranges = Vec::with_capacity(12);
            for (name, expected, transpose) in specs {
                let vals = read_tensor(&name, &expected, transpose)?;
                ranges.push(push_aligned(&vals));
            }
            layers.push(layer_from(&ranges));
        }
        let lnf_w = push_aligned(&read_tensor("output_norm.weight", &[e], false)?);
        let lnf_b = push_aligned(&read_tensor("output_norm.bias", &[e], false)?);

        // Untied models carry a separate LM head with the same `{n_embd,
        // n_vocab}` layout as the token embedding; tied models omit it.
        let wte_t_vals = if infos.contains_key("output.weight") {
            read_tensor("output.weight", &[e, v], true)?
        } else {
            transpose_2d(&wte_vals, v, e)
        };
        let wte_t = push_aligned(&wte_t_vals);

        let mut vocab_map = serde_json::Map::with_capacity(tokens.len());
        for (idx, tok) in tokens.into_iter().enumerate() {
            vocab_map.insert(tok, serde_json::Value::from(idx));
        }
        let vocab_json = serde_json::to_string(&serde_json::Value::Object(vocab_map))
            .map_err(|e| invalid(e.to_string()))?;
        let mut merges_txt = String::from("#version: 0.2\n");
        for line in merges {
            merges_txt.push_str(&line);
            merges_txt.push('\n');
        }

        let model = Self::build_q8(
            cfg,
            Store::Owned(flat),
            wte,
            wte_t,
            wpe,
            layers,
            lnf_w,
            lnf_b,
        );
        Ok((model, vocab_json, merges_txt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }

    fn push_kv_str(buf: &mut Vec<u8>, k: &str, v: &str) {
        push_str(buf, k);
        buf.extend_from_slice(&META_STRING.to_le_bytes());
        push_str(buf, v);
    }

    fn push_kv_u32(buf: &mut Vec<u8>, k: &str, v: u32) {
        push_str(buf, k);
        buf.extend_from_slice(&META_U32.to_le_bytes());
        buf.extend_from_slice(&v.to_le_bytes());
    }

    fn push_kv_str_arr(buf: &mut Vec<u8>, k: &str, items: &[&str]) {
        push_str(buf, k);
        buf.extend_from_slice(&META_ARRAY.to_le_bytes());
        buf.extend_from_slice(&META_STRING.to_le_bytes());
        buf.extend_from_slice(&(items.len() as u64).to_le_bytes());
        for s in items {
            push_str(buf, s);
        }
    }

    const E: usize = 4;
    const H: usize = 2;
    const L: usize = 1;
    const P: usize = 8;
    const V: usize = 4;

    fn base_tensors() -> Vec<(&'static str, Vec<usize>, Vec<f32>)> {
        let (e, p, v) = (E, P, V);
        vec![
            ("token_embd.weight", vec![e, v], vec![0.1; v * e]),
            ("position_embd.weight", vec![e, p], vec![0.05; p * e]),
            ("blk.0.attn_norm.weight", vec![e], vec![1.0; e]),
            ("blk.0.attn_norm.bias", vec![e], vec![0.0; e]),
            (
                "blk.0.attn_qkv.weight",
                vec![e, 3 * e],
                vec![0.02; 3 * e * e],
            ),
            ("blk.0.attn_qkv.bias", vec![3 * e], vec![0.0; 3 * e]),
            ("blk.0.attn_output.weight", vec![e, e], vec![0.02; e * e]),
            ("blk.0.attn_output.bias", vec![e], vec![0.0; e]),
            ("blk.0.ffn_norm.weight", vec![e], vec![1.0; e]),
            ("blk.0.ffn_norm.bias", vec![e], vec![0.0; e]),
            ("blk.0.ffn_up.weight", vec![e, 4 * e], vec![0.02; 4 * e * e]),
            ("blk.0.ffn_up.bias", vec![4 * e], vec![0.0; 4 * e]),
            (
                "blk.0.ffn_down.weight",
                vec![4 * e, e],
                vec![0.02; e * 4 * e],
            ),
            ("blk.0.ffn_down.bias", vec![e], vec![0.0; e]),
            ("output_norm.weight", vec![e], vec![1.0; e]),
            ("output_norm.bias", vec![e], vec![0.0; e]),
        ]
    }

    fn build_gguf(tensor_specs: &[(&str, Vec<usize>, Vec<f32>)], head_count: u32) -> Vec<u8> {
        let (e, l, p) = (E, L, P);
        let mut buf = Vec::new();
        buf.extend_from_slice(GGUF_MAGIC);
        buf.extend_from_slice(&3u32.to_le_bytes()); // version 3
        buf.extend_from_slice(&(tensor_specs.len() as u64).to_le_bytes());
        buf.extend_from_slice(&7u64.to_le_bytes()); // 7 metadata KV entries

        push_kv_str(&mut buf, "general.architecture", "gpt2");
        push_kv_u32(&mut buf, "gpt2.embedding_length", e as u32);
        push_kv_u32(&mut buf, "gpt2.attention.head_count", head_count);
        push_kv_u32(&mut buf, "gpt2.block_count", l as u32);
        push_kv_u32(&mut buf, "gpt2.context_length", p as u32);
        push_kv_str_arr(&mut buf, "tokenizer.ggml.tokens", &["a", "b", "ab", "</s>"]);
        push_kv_str_arr(&mut buf, "tokenizer.ggml.merges", &["a b"]);

        let mut data_section = Vec::new();
        for (name, dims, vals) in tensor_specs {
            let offset = data_section.len();
            push_str(&mut buf, name);
            buf.extend_from_slice(&(dims.len() as u32).to_le_bytes());
            for &d in dims {
                buf.extend_from_slice(&(d as u64).to_le_bytes());
            }
            buf.extend_from_slice(&GGML_TYPE_F32.to_le_bytes());
            buf.extend_from_slice(&(offset as u64).to_le_bytes());
            for &f in vals {
                data_section.extend_from_slice(&f.to_le_bytes());
            }
            data_section.resize(data_section.len().div_ceil(32) * 32, 0);
        }
        buf.resize(buf.len().div_ceil(32) * 32, 0);
        buf.extend_from_slice(&data_section);
        buf
    }

    #[test]
    fn loads_minimal_gpt2_gguf_and_packs_to_swz() {
        let buf = build_gguf(&base_tensors(), H as u32);
        let (model, vocab, merges) = Model::load_gguf_bytes(&buf).unwrap();
        assert_eq!(model.cfg.n_embd, E);
        assert_eq!(model.cfg.n_head, H);
        assert_eq!(model.cfg.n_layer, L);
        assert_eq!(model.cfg.vocab_size, V);
        // Tied model: the LM head is the transposed token embedding.
        let d = model.store.data();
        assert!(
            d[model.wte_t.clone()]
                .iter()
                .all(|&x| (x - 0.1).abs() < 1e-6)
        );
        assert!(vocab.contains("\"ab\":2"));
        assert!(merges.contains("a b"));

        // Verify Q8_0 dequantization helper too.
        let mut q8_block = [0u8; 34];
        q8_block[0..2].copy_from_slice(&0x3C00u16.to_le_bytes()); // f16 1.0
        for i in 0..32 {
            q8_block[2 + i] = (i as i8 - 16) as u8;
        }
        let deq = dequantize(&q8_block, GGML_TYPE_Q8_0, 32).unwrap();
        assert!((deq[0] - (-16.0)).abs() < 1e-4);
        assert!((deq[16] - 0.0).abs() < 1e-4);
        assert!((deq[31] - 15.0).abs() < 1e-4);
    }

    #[test]
    fn uses_untied_output_weight_for_lm_head() {
        let mut specs = base_tensors();
        let out: Vec<f32> = (0..V * E).map(|i| i as f32).collect();
        specs.push(("output.weight", vec![E, V], out.clone()));
        let buf = build_gguf(&specs, H as u32);
        let (model, _, _) = Model::load_gguf_bytes(&buf).unwrap();
        let d = model.store.data();
        let head = &d[model.wte_t.clone()];
        // output.weight is row-major [V, E]; the LM head is stored as [E, V].
        for r in 0..V {
            for c in 0..E {
                assert_eq!(head[c * V + r], out[r * E + c]);
            }
        }
    }

    #[test]
    fn rejects_invalid_head_count_and_misshaped_tensor() {
        assert!(Model::load_gguf_bytes(&build_gguf(&base_tensors(), 0)).is_err());
        assert!(Model::load_gguf_bytes(&build_gguf(&base_tensors(), 3)).is_err());
        let mut specs = base_tensors();
        // Same element count as [E, 3E] but wrong orientation.
        specs[4].1 = vec![3 * E, E];
        assert!(Model::load_gguf_bytes(&build_gguf(&specs, H as u32)).is_err());
    }

    #[test]
    fn rejects_oversized_counts_and_deep_nesting() {
        let mut buf = Vec::new();
        buf.extend_from_slice(GGUF_MAGIC);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&u64::MAX.to_le_bytes()); // tensor count
        buf.extend_from_slice(&1u64.to_le_bytes()); // kv count
        push_str(&mut buf, "tokenizer.ggml.tokens");
        buf.extend_from_slice(&META_ARRAY.to_le_bytes());
        buf.extend_from_slice(&META_STRING.to_le_bytes());
        buf.extend_from_slice(&(u64::MAX / 2).to_le_bytes());
        assert!(Model::load_gguf_bytes(&buf).is_err());

        let mut cur_bytes = Vec::new();
        for _ in 0..(MAX_META_DEPTH + 2) {
            cur_bytes.extend_from_slice(&META_ARRAY.to_le_bytes());
            cur_bytes.extend_from_slice(&1u64.to_le_bytes());
        }
        let mut cur = Cursor::new(&cur_bytes);
        assert!(cur.skip_value(META_ARRAY, 0).is_err());
    }

    #[test]
    fn reports_big_endian_files() {
        let mut buf = Vec::new();
        buf.extend_from_slice(GGUF_MAGIC);
        buf.extend_from_slice(&3u32.to_be_bytes());
        let err = Model::load_gguf_bytes(&buf).err().unwrap();
        assert!(err.to_string().contains("big-endian"));
    }
}
