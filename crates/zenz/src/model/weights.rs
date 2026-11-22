//! Hugging Face safetensors and the packed SWZ model format.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use super::{Config, Layer, Model, R, Store};

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

const LAYER_TENSORS: [&str; 12] = [
    "ln_1.weight",
    "ln_1.bias",
    "attn.c_attn.weight",
    "attn.c_attn.bias",
    "attn.c_proj.weight",
    "attn.c_proj.bias",
    "ln_2.weight",
    "ln_2.bias",
    "mlp.c_fc.weight",
    "mlp.c_fc.bias",
    "mlp.c_proj.weight",
    "mlp.c_proj.bias",
];

const SWZ_MAGIC: &[u8; 8] = b"SWZENZ01";

fn load_tensors(path: &Path) -> io::Result<HashMap<String, (Vec<usize>, Vec<f32>)>> {
    let data = std::fs::read(path)?;
    if data.len() < 8 {
        return Err(invalid("safetensors too small"));
    }
    let n = u64::from_le_bytes(data[..8].try_into().unwrap()) as usize;
    let header: serde_json::Value =
        serde_json::from_slice(&data[8..8 + n]).map_err(|e| invalid(e.to_string()))?;
    let body = &data[8 + n..];
    let mut out = HashMap::new();
    for (name, info) in header.as_object().ok_or_else(|| invalid("bad header"))? {
        if name == "__metadata__" {
            continue;
        }
        let dtype = info["dtype"].as_str().unwrap_or("");
        let shape: Vec<usize> = info["shape"]
            .as_array()
            .ok_or_else(|| invalid("bad shape"))?
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as usize)
            .collect();
        let off = info["data_offsets"]
            .as_array()
            .ok_or_else(|| invalid("bad offsets"))?;
        let (s, e) = (
            off[0].as_u64().unwrap() as usize,
            off[1].as_u64().unwrap() as usize,
        );
        let raw = body
            .get(s..e)
            .ok_or_else(|| invalid("tensor out of range"))?;
        let vals: Vec<f32> = match dtype {
            "F32" => raw
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
            "BF16" => raw
                .chunks_exact(2)
                .map(|b| f32::from_bits((u16::from_le_bytes([b[0], b[1]]) as u32) << 16))
                .collect(),
            "F16" => raw
                .chunks_exact(2)
                .map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]])))
                .collect(),
            other => return Err(invalid(format!("unsupported dtype {other}"))),
        };
        out.insert(name.clone(), (shape, vals));
    }
    Ok(out)
}

fn f16_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) as u32) << 31;
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s
        } else {
            // subnormal
            let mut e = -1i32;
            let mut m = m;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            s | (((e + 127 - 14) as u32) << 23) | ((m & 0x3ff) << 13)
        }
    } else if e == 31 {
        s | 0x7f80_0000 | (m << 13)
    } else {
        s | (((e - 15 + 127) as u32) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

fn layer_from(r: &[R]) -> Layer {
    Layer {
        ln1_w: r[0].clone(),
        ln1_b: r[1].clone(),
        attn_w: r[2].clone(),
        attn_b: r[3].clone(),
        proj_w: r[4].clone(),
        proj_b: r[5].clone(),
        ln2_w: r[6].clone(),
        ln2_b: r[7].clone(),
        fc_w: r[8].clone(),
        fc_b: r[9].clone(),
        fc2_w: r[10].clone(),
        fc2_b: r[11].clone(),
    }
}

impl Model {
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let cfg_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)
                .map_err(|e| invalid(e.to_string()))?;
        let get = |k: &str| {
            cfg_json[k]
                .as_u64()
                .map(|v| v as usize)
                .ok_or_else(|| invalid(format!("config {k}")))
        };
        let cfg = Config {
            n_embd: get("n_embd")?,
            n_head: get("n_head")?,
            n_layer: get("n_layer")?,
            n_positions: get("n_positions")?,
            vocab_size: get("vocab_size")?,
            eps: cfg_json["layer_norm_epsilon"].as_f64().unwrap_or(1e-5) as f32,
        };
        let mut t = load_tensors(&dir.join("model.safetensors"))?;
        let mut data: Vec<f32> = Vec::new();
        let mut take = |name: &str| -> io::Result<R> {
            let v = t
                .remove(&format!("transformer.{name}"))
                .or_else(|| t.remove(name))
                .map(|(_, v)| v)
                .ok_or_else(|| invalid(format!("missing tensor {name}")))?;
            let start = data.len();
            data.extend_from_slice(&v);
            // keep every tensor 16-float (64-byte) aligned
            data.resize(data.len().div_ceil(16) * 16, 0.0);
            Ok(start..start + v.len())
        };
        let wte = take("wte.weight")?;
        let wpe = take("wpe.weight")?;
        let mut layers = Vec::new();
        for i in 0..cfg.n_layer {
            let mut r: Vec<R> = Vec::new();
            for n in LAYER_TENSORS {
                r.push(take(&format!("h.{i}.{n}"))?);
            }
            layers.push(layer_from(&r));
        }
        let lnf_w = take("ln_f.weight")?;
        let lnf_b = take("ln_f.bias")?;
        let (v, e) = (cfg.vocab_size, cfg.n_embd);
        if wte.len() != v * e {
            return Err(invalid("wte shape mismatch"));
        }
        let start = data.len();
        data.resize(start + v * e, 0.0);
        for r in 0..v {
            for c in 0..e {
                data[start + c * v + r] = data[wte.start + r * e + c];
            }
        }
        let wte_t = start..start + v * e;
        Ok(Self {
            cfg,
            store: Store::Owned(data),
            wte,
            wte_t,
            wpe,
            layers,
            lnf_w,
            lnf_b,
        })
    }

    fn ranges(&self) -> Vec<(String, R)> {
        let mut v = vec![
            ("wte".to_string(), self.wte.clone()),
            ("wte_t".to_string(), self.wte_t.clone()),
            ("wpe".to_string(), self.wpe.clone()),
            ("lnf_w".to_string(), self.lnf_w.clone()),
            ("lnf_b".to_string(), self.lnf_b.clone()),
        ];
        for (i, l) in self.layers.iter().enumerate() {
            let rs = [
                &l.ln1_w, &l.ln1_b, &l.attn_w, &l.attn_b, &l.proj_w, &l.proj_b, &l.ln2_w, &l.ln2_b,
                &l.fc_w, &l.fc_b, &l.fc2_w, &l.fc2_b,
            ];
            for (n, r) in LAYER_TENSORS.iter().zip(rs) {
                v.push((format!("h.{i}.{n}"), r.clone()));
            }
        }
        v
    }

    /// Writes a packed, mmap-able model file including the tokenizer files.
    pub fn pack(&self, vocab_json: &str, merges_txt: &str, out: &Path) -> io::Result<()> {
        let c = &self.cfg;
        let tensors: serde_json::Map<String, serde_json::Value> = self
            .ranges()
            .into_iter()
            .map(|(n, r)| (n, serde_json::json!([r.start, r.end])))
            .collect();
        let header = serde_json::json!({
            "n_embd": c.n_embd, "n_head": c.n_head, "n_layer": c.n_layer,
            "n_positions": c.n_positions, "vocab_size": c.vocab_size, "eps": c.eps,
            "tensors": tensors, "vocab": vocab_json, "merges": merges_txt,
        });
        let hb = serde_json::to_vec(&header).map_err(|e| invalid(e.to_string()))?;
        let mut buf = Vec::new();
        buf.extend_from_slice(SWZ_MAGIC);
        buf.extend_from_slice(&(hb.len() as u64).to_le_bytes());
        buf.extend_from_slice(&hb);
        buf.resize(buf.len().div_ceil(64) * 64, 0);
        let data = self.store.data();
        buf.extend_from_slice(&(data.len() as u64).to_le_bytes());
        buf.resize(buf.len().div_ceil(64) * 64, 0);
        for f in data {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        std::fs::write(out, buf)
    }

    /// Loads a packed `.swz` file. Returns the model and (vocab.json, merges.txt).
    pub fn load_packed(path: &Path) -> io::Result<(Self, String, String)> {
        let file = std::fs::File::open(path)?;
        // SAFETY: the file is opened read-only and must not change while mapped.
        let map = unsafe { memmap2::Mmap::map(&file)? };
        if map.len() < 16 || &map[..8] != SWZ_MAGIC {
            return Err(invalid("not a .swz model"));
        }
        let hl = u64::from_le_bytes(map[8..16].try_into().unwrap()) as usize;
        let hdr: serde_json::Value =
            serde_json::from_slice(map.get(16..16 + hl).ok_or_else(|| invalid("truncated"))?)
                .map_err(|e| invalid(e.to_string()))?;
        let mut off = (16 + hl).div_ceil(64) * 64;
        let n = u64::from_le_bytes(
            map.get(off..off + 8)
                .ok_or_else(|| invalid("truncated"))?
                .try_into()
                .unwrap(),
        ) as usize;
        off = (off + 8).div_ceil(64) * 64;
        if off + n * 4 > map.len() {
            return Err(invalid("truncated weights"));
        }
        let g = |k: &str| {
            hdr[k]
                .as_u64()
                .map(|v| v as usize)
                .ok_or_else(|| invalid(format!("header {k}")))
        };
        let cfg = Config {
            n_embd: g("n_embd")?,
            n_head: g("n_head")?,
            n_layer: g("n_layer")?,
            n_positions: g("n_positions")?,
            vocab_size: g("vocab_size")?,
            eps: hdr["eps"].as_f64().unwrap_or(1e-5) as f32,
        };
        let r = |name: &str| -> io::Result<R> {
            let a = hdr["tensors"][name]
                .as_array()
                .ok_or_else(|| invalid(format!("tensor {name}")))?;
            let (s, e) = (
                a[0].as_u64().unwrap_or(0) as usize,
                a[1].as_u64().unwrap_or(0) as usize,
            );
            if s > e || e > n {
                return Err(invalid(format!("tensor {name} out of range")));
            }
            Ok(s..e)
        };
        let mut layers = Vec::new();
        for i in 0..cfg.n_layer {
            let rs: Vec<R> = LAYER_TENSORS
                .iter()
                .map(|t| r(&format!("h.{i}.{t}")))
                .collect::<io::Result<_>>()?;
            layers.push(layer_from(&rs));
        }
        let vocab = hdr["vocab"].as_str().unwrap_or("").to_string();
        let merges = hdr["merges"].as_str().unwrap_or("").to_string();
        let model = Self {
            cfg,
            wte: r("wte")?,
            wte_t: r("wte_t")?,
            wpe: r("wpe")?,
            lnf_w: r("lnf_w")?,
            lnf_b: r("lnf_b")?,
            layers,
            store: Store::Mapped {
                map,
                offset: off,
                len: n,
            },
        };
        Ok((model, vocab, merges))
    }
}
