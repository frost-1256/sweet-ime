//! GPT-2 forward pass with a KV cache, loading HF safetensors (F32/BF16/F16).

use std::collections::HashMap;
use std::io;
use std::path::Path;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub n_embd: usize,
    pub n_head: usize,
    pub n_layer: usize,
    pub n_positions: usize,
    pub vocab_size: usize,
    pub eps: f32,
}

type R = std::ops::Range<usize>;

/// Weight ranges of one transformer block inside the flat parameter buffer.
#[derive(Clone)]
struct Layer {
    ln1_w: R,
    ln1_b: R,
    attn_w: R, // [E, 3E]
    attn_b: R,
    proj_w: R, // [E, E]
    proj_b: R,
    ln2_w: R,
    ln2_b: R,
    fc_w: R, // [E, 4E]
    fc_b: R,
    fc2_w: R, // [4E, E]
    fc2_b: R,
}

const LAYER_TENSORS: [&str; 12] = [
    "ln_1.weight", "ln_1.bias", "attn.c_attn.weight", "attn.c_attn.bias", "attn.c_proj.weight",
    "attn.c_proj.bias", "ln_2.weight", "ln_2.bias", "mlp.c_fc.weight", "mlp.c_fc.bias",
    "mlp.c_proj.weight", "mlp.c_proj.bias",
];

struct LayerView<'a> {
    ln1_w: &'a [f32],
    ln1_b: &'a [f32],
    attn_w: &'a [f32],
    attn_b: &'a [f32],
    proj_w: &'a [f32],
    proj_b: &'a [f32],
    ln2_w: &'a [f32],
    ln2_b: &'a [f32],
    fc_w: &'a [f32],
    fc_b: &'a [f32],
    fc2_w: &'a [f32],
    fc2_b: &'a [f32],
}

/// Flat parameter storage: owned (from safetensors) or memory-mapped from a
/// packed `.swz` file, which lets every process share one copy.
enum Store {
    Owned(Vec<f32>),
    Mapped { map: memmap2::Mmap, offset: usize, len: usize },
}

impl Store {
    fn data(&self) -> &[f32] {
        match self {
            Store::Owned(v) => v,
            Store::Mapped { map, offset, len } => {
                let bytes = &map[*offset..*offset + *len * 4];
                // SAFETY: offset is 64-byte aligned within a page-aligned
                // mapping and the length was validated at load time.
                unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, *len) }
            }
        }
    }
}

pub struct Model {
    pub cfg: Config,
    store: Store,
    wte: R,   // [V, E]
    wte_t: R, // [E, V] for the LM head
    wpe: R,   // [P, E]
    layers: Vec<Layer>,
    lnf_w: R,
    lnf_b: R,
}

const SWZ_MAGIC: &[u8; 8] = b"SWZENZ01";

/// Per-sequence key/value cache.
#[derive(Clone)]
pub struct KvCache {
    k: Vec<Vec<f32>>, // per layer, [len, E]
    v: Vec<Vec<f32>>,
    pub len: usize,
}

impl KvCache {
    pub fn truncate(&mut self, len: usize) {
        let e = if self.len == 0 { 0 } else { self.k[0].len() / self.len };
        for l in 0..self.k.len() {
            self.k[l].truncate(len * e);
            self.v[l].truncate(len * e);
        }
        self.len = len.min(self.len);
    }
}

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
        let off = info["data_offsets"].as_array().ok_or_else(|| invalid("bad offsets"))?;
        let (s, e) = (off[0].as_u64().unwrap() as usize, off[1].as_u64().unwrap() as usize);
        let raw = body.get(s..e).ok_or_else(|| invalid("tensor out of range"))?;
        let vals: Vec<f32> = match dtype {
            "F32" => raw.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect(),
            "BF16" => raw
                .chunks_exact(2)
                .map(|b| f32::from_bits((u16::from_le_bytes([b[0], b[1]]) as u32) << 16))
                .collect(),
            "F16" => raw.chunks_exact(2).map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]]))).collect(),
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

use crate::kernels::matmul;

fn add_bias(y: &mut [f32], b: &[f32]) {
    for row in y.chunks_exact_mut(b.len()) {
        for (v, bb) in row.iter_mut().zip(b) {
            *v += bb;
        }
    }
}

fn layer_norm(x: &[f32], w: &[f32], b: &[f32], eps: f32, out: &mut [f32]) {
    let e = w.len();
    for (xr, or) in x.chunks_exact(e).zip(out.chunks_exact_mut(e)) {
        let mean = xr.iter().sum::<f32>() / e as f32;
        let var = xr.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / e as f32;
        let inv = 1.0 / (var + eps).sqrt();
        for i in 0..e {
            or[i] = (xr[i] - mean) * inv * w[i] + b[i];
        }
    }
}

fn gelu_new(v: &mut [f32]) {
    const C: f32 = 0.797_884_6; // sqrt(2/pi)
    for x in v {
        let t = (C * (*x + 0.044715 * *x * *x * *x)).tanh();
        *x = 0.5 * *x * (1.0 + t);
    }
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
        let get = |k: &str| cfg_json[k].as_u64().map(|v| v as usize).ok_or_else(|| invalid(format!("config {k}")));
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
        Ok(Self { cfg, store: Store::Owned(data), wte, wte_t, wpe, layers, lnf_w, lnf_b })
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
        let n = u64::from_le_bytes(map.get(off..off + 8).ok_or_else(|| invalid("truncated"))?.try_into().unwrap()) as usize;
        off = (off + 8).div_ceil(64) * 64;
        if off + n * 4 > map.len() {
            return Err(invalid("truncated weights"));
        }
        let g = |k: &str| hdr[k].as_u64().map(|v| v as usize).ok_or_else(|| invalid(format!("header {k}")));
        let cfg = Config {
            n_embd: g("n_embd")?,
            n_head: g("n_head")?,
            n_layer: g("n_layer")?,
            n_positions: g("n_positions")?,
            vocab_size: g("vocab_size")?,
            eps: hdr["eps"].as_f64().unwrap_or(1e-5) as f32,
        };
        let r = |name: &str| -> io::Result<R> {
            let a = hdr["tensors"][name].as_array().ok_or_else(|| invalid(format!("tensor {name}")))?;
            let (s, e) = (a[0].as_u64().unwrap_or(0) as usize, a[1].as_u64().unwrap_or(0) as usize);
            if s > e || e > n {
                return Err(invalid(format!("tensor {name} out of range")));
            }
            Ok(s..e)
        };
        let mut layers = Vec::new();
        for i in 0..cfg.n_layer {
            let rs: Vec<R> = LAYER_TENSORS.iter().map(|t| r(&format!("h.{i}.{t}"))).collect::<io::Result<_>>()?;
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
            store: Store::Mapped { map, offset: off, len: n },
        };
        Ok((model, vocab, merges))
    }

    fn view<'a>(&'a self, d: &'a [f32], l: &Layer) -> LayerView<'a> {
        LayerView {
            ln1_w: &d[l.ln1_w.clone()],
            ln1_b: &d[l.ln1_b.clone()],
            attn_w: &d[l.attn_w.clone()],
            attn_b: &d[l.attn_b.clone()],
            proj_w: &d[l.proj_w.clone()],
            proj_b: &d[l.proj_b.clone()],
            ln2_w: &d[l.ln2_w.clone()],
            ln2_b: &d[l.ln2_b.clone()],
            fc_w: &d[l.fc_w.clone()],
            fc_b: &d[l.fc_b.clone()],
            fc2_w: &d[l.fc2_w.clone()],
            fc2_b: &d[l.fc2_b.clone()],
        }
    }

    pub fn new_cache(&self) -> KvCache {
        KvCache { k: vec![Vec::new(); self.cfg.n_layer], v: vec![Vec::new(); self.cfg.n_layer], len: 0 }
    }

    /// Runs `tokens` after the cached prefix and appends them to the cache.
    /// Returns logits for every position (`want_all`) or only the last one.
    pub fn forward(&self, tokens: &[u32], cache: &mut KvCache, want_all: bool) -> Vec<f32> {
        let parents: Vec<Option<usize>> = (0..tokens.len()).map(|i| i.checked_sub(1)).collect();
        self.forward_impl(tokens, &parents, cache, true, if want_all { 0 } else { tokens.len() - 1 })
    }

    /// Tree attention: evaluates a whole token trie in one pass. `parents[i]`
    /// is the index (within `tokens`) of node i's parent, or `None` when it
    /// directly follows the cached prefix. Parents must precede children.
    /// The cache is left unchanged. Returns logits `[tokens.len() - logits_from, V]`
    /// for the nodes from index `logits_from` on.
    pub fn forward_tree(&self, tokens: &[u32], parents: &[Option<usize>], cache: &mut KvCache, logits_from: usize) -> Vec<f32> {
        self.forward_impl(tokens, parents, cache, false, logits_from)
    }

    fn forward_impl(
        &self,
        tokens: &[u32],
        parents: &[Option<usize>],
        cache: &mut KvCache,
        append: bool,
        logits_from: usize,
    ) -> Vec<f32> {
        let Config { n_embd: e, n_head: nh, eps, vocab_size: vsz, .. } = self.cfg;
        let t = tokens.len();
        if t == 0 {
            return Vec::new();
        }
        let past = cache.len;
        let hd = e / nh;
        let scale = 1.0 / (hd as f32).sqrt();
        // Ancestor chains (root..=self) within the batch.
        let mut anc: Vec<Vec<usize>> = Vec::with_capacity(t);
        for i in 0..t {
            let mut a = match parents[i] {
                Some(p) => {
                    assert!(p < i, "parent must precede child");
                    anc[p].clone()
                }
                None => Vec::new(),
            };
            a.push(i);
            anc.push(a);
        }
        let d = self.store.data();
        let (wte, wpe) = (&d[self.wte.clone()], &d[self.wpe.clone()]);
        let mut x = vec![0f32; t * e];
        for (i, &tok) in tokens.iter().enumerate() {
            let pos = (past + anc[i].len() - 1).min(self.cfg.n_positions - 1);
            let te = &wte[tok as usize * e..][..e];
            let pe = &wpe[pos * e..][..e];
            for j in 0..e {
                x[i * e + j] = te[j] + pe[j];
            }
        }
        let mut h = vec![0f32; t * e];
        let mut qkv = vec![0f32; t * 3 * e];
        let mut att = vec![0f32; t * e];
        let mut ff = vec![0f32; t * 4 * e];
        let max_ctx = past + anc.iter().map(|a| a.len()).max().unwrap_or(0);
        let mut scores = vec![0f32; max_ctx];
        for (li, lr) in self.layers.iter().enumerate() {
            let l = self.view(d, lr);
            layer_norm(&x, &l.ln1_w, &l.ln1_b, eps, &mut h);
            matmul(&h, &l.attn_w, &mut qkv, t, e, 3 * e, false);
            add_bias(&mut qkv, &l.attn_b);
            {
                let kc = &cache.k[li];
                let vc = &cache.v[li];
                for i in 0..t {
                    let n_ctx = past + anc[i].len();
                    for hh in 0..nh {
                        let off = hh * hd;
                        let q = &qkv[i * 3 * e + off..][..hd];
                        let mut mx = f32::NEG_INFINITY;
                        for j in 0..n_ctx {
                            let k: &[f32] = if j < past {
                                &kc[j * e + off..][..hd]
                            } else {
                                &qkv[anc[i][j - past] * 3 * e + e + off..][..hd]
                            };
                            let s = q.iter().zip(k).map(|(a, b)| a * b).sum::<f32>() * scale;
                            scores[j] = s;
                            mx = mx.max(s);
                        }
                        let mut sum = 0.0;
                        for s in &mut scores[..n_ctx] {
                            *s = (*s - mx).exp();
                            sum += *s;
                        }
                        let out = &mut att[i * e + off..][..hd];
                        out.fill(0.0);
                        for j in 0..n_ctx {
                            let w = scores[j] / sum;
                            let v: &[f32] = if j < past {
                                &vc[j * e + off..][..hd]
                            } else {
                                &qkv[anc[i][j - past] * 3 * e + 2 * e + off..][..hd]
                            };
                            for d in 0..hd {
                                out[d] += w * v[d];
                            }
                        }
                    }
                }
            }
            if append {
                for i in 0..t {
                    cache.k[li].extend_from_slice(&qkv[i * 3 * e + e..i * 3 * e + 2 * e]);
                    cache.v[li].extend_from_slice(&qkv[i * 3 * e + 2 * e..i * 3 * e + 3 * e]);
                }
            }
            matmul(&att, &l.proj_w, &mut x, t, e, e, true);
            add_bias(&mut x, &l.proj_b);
            layer_norm(&x, &l.ln2_w, &l.ln2_b, eps, &mut h);
            matmul(&h, &l.fc_w, &mut ff, t, e, 4 * e, false);
            add_bias(&mut ff, &l.fc_b);
            gelu_new(&mut ff);
            matmul(&ff, &l.fc2_w, &mut x, t, 4 * e, e, true);
            add_bias(&mut x, &l.fc2_b);
        }
        if append {
            cache.len = past + t;
        }
        let rows = t - logits_from.min(t - 1);
        let src = &x[(t - rows) * e..];
        let mut hf = vec![0f32; rows * e];
        layer_norm(src, &d[self.lnf_w.clone()], &d[self.lnf_b.clone()], eps, &mut hf);
        let mut logits = vec![0f32; rows * vsz];
        matmul(&hf, &d[self.wte_t.clone()], &mut logits, rows, e, vsz, false);
        logits
    }
}

/// log-softmax value of `idx` within `logits`.
pub fn log_prob(logits: &[f32], idx: usize) -> f32 {
    let mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = logits.iter().map(|v| (v - mx).exp()).sum();
    logits[idx] - mx - sum.ln()
}

pub fn argmax(logits: &[f32]) -> usize {
    let mut best = 0;
    for (i, v) in logits.iter().enumerate() {
        if *v > logits[best] {
            best = i;
        }
    }
    best
}
