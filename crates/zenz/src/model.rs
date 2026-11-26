//! GPT-2 forward pass with a KV cache, loading HF safetensors (F32/BF16/F16).

use std::cell::RefCell;

use crate::kernels::{axpy_f32, dot_f32, matmul, matmul_bias};

mod weights;

#[derive(Default)]
struct ForwardScratch {
    x: Vec<f32>,
    h: Vec<f32>,
    qkv: Vec<f32>,
    att: Vec<f32>,
    ff: Vec<f32>,
    scores: Vec<f32>,
    hf: Vec<f32>,
}

impl ForwardScratch {
    fn trim(&mut self, max_elems: usize) {
        for buf in [
            &mut self.x,
            &mut self.h,
            &mut self.qkv,
            &mut self.att,
            &mut self.ff,
            &mut self.hf,
        ] {
            if buf.capacity() > max_elems {
                buf.clear();
                buf.shrink_to(max_elems);
            }
        }
    }
}

thread_local! {
    static FORWARD_SCRATCH: RefCell<ForwardScratch> = RefCell::new(ForwardScratch::default());
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
    if v.len() >= 16_384 {
        use rayon::prelude::*;
        crate::kernels::pool().install(|| v.par_chunks_mut(4096).for_each(gelu_chunk));
    } else {
        gelu_chunk(v);
    }
}

fn gelu_chunk(v: &mut [f32]) {
    // (1 + tanh(z)) / 2 = 1 / (1 + exp(-2z)).
    const C: f32 = -1.595_769_2; // -2 * sqrt(2/pi)
    for x in v {
        let exponent = C * (*x + 0.044715 * *x * *x * *x);
        *x /= 1.0 + exponent.exp();
    }
}

impl Model {
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
        // Keep the layer work on the same workers as the matrix products.
        crate::kernels::pool().install(|| self.forward_impl(tokens, parents, cache, false, logits_from))
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
        let max_ctx = past + anc.iter().map(|a| a.len()).max().unwrap_or(0);
        let rows = t - logits_from.min(t - 1);
        let mut logits = vec![0f32; rows * vsz];
        FORWARD_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            let ForwardScratch {
                x,
                h,
                qkv,
                att,
                ff,
                scores,
                hf,
            } = &mut *scratch;
            x.resize(t * e, 0.0);
            h.resize(t * e, 0.0);
            qkv.resize(t * 3 * e, 0.0);
            att.resize(t * e, 0.0);
            ff.resize(t * 4 * e, 0.0);
            scores.resize(max_ctx, 0.0);
            hf.resize(rows * e, 0.0);
            for (i, &tok) in tokens.iter().enumerate() {
                let pos = (past + anc[i].len() - 1).min(self.cfg.n_positions - 1);
                let te = &wte[tok as usize * e..][..e];
                let pe = &wpe[pos * e..][..e];
                for j in 0..e {
                    x[i * e + j] = te[j] + pe[j];
                }
            }
            for (li, lr) in self.layers.iter().enumerate() {
                let l = self.view(d, lr);
                layer_norm(x, &l.ln1_w, &l.ln1_b, eps, h);
                matmul_bias(h, &l.attn_w, Some(&l.attn_b), qkv, t, e, 3 * e, false);
                {
                    let kc = &cache.k[li];
                    let vc = &cache.v[li];
                    let run_row = |i: usize, att_row: &mut [f32], row_scores: &mut [f32]| {
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
                                let s = dot_f32(q, k) * scale;
                                row_scores[j] = s;
                                mx = mx.max(s);
                            }
                            let mut sum = 0.0;
                            for s in &mut row_scores[..n_ctx] {
                                *s = (*s - mx).exp();
                                sum += *s;
                            }
                            let out = &mut att_row[off..][..hd];
                            out.fill(0.0);
                            let inv_sum = sum.recip();
                            for j in 0..n_ctx {
                                let w = row_scores[j] * inv_sum;
                                let v: &[f32] = if j < past {
                                    &vc[j * e + off..][..hd]
                                } else {
                                    &qkv[anc[i][j - past] * 3 * e + 2 * e + off..][..hd]
                                };
                                axpy_f32(out, v, w);
                            }
                        }
                    };
                    if t >= 12 {
                        use rayon::prelude::*;
                        att.par_chunks_exact_mut(e)
                            .enumerate()
                            .for_each_init(
                                || vec![0f32; max_ctx],
                                |row_scores, (i, att_row)| run_row(i, att_row, row_scores),
                            );
                    } else {
                        for (i, att_row) in att.chunks_exact_mut(e).enumerate() {
                            run_row(i, att_row, scores);
                        }
                    }
                }
                if append {
                    for i in 0..t {
                        cache.k[li].extend_from_slice(&qkv[i * 3 * e + e..i * 3 * e + 2 * e]);
                        cache.v[li].extend_from_slice(&qkv[i * 3 * e + 2 * e..i * 3 * e + 3 * e]);
                    }
                }
                matmul_bias(att, &l.proj_w, Some(&l.proj_b), x, t, e, e, true);
                layer_norm(x, &l.ln2_w, &l.ln2_b, eps, h);
                matmul_bias(h, &l.fc_w, Some(&l.fc_b), ff, t, e, 4 * e, false);
                gelu_new(ff);
                matmul_bias(ff, &l.fc2_w, Some(&l.fc2_b), x, t, 4 * e, e, true);
            }
            let src = &x[(t - rows) * e..];
            layer_norm(src, &d[self.lnf_w.clone()], &d[self.lnf_b.clone()], eps, hf);
            matmul(hf, &d[self.wte_t.clone()], &mut logits, rows, e, vsz, false);
            // Keep steady-state memory bounded after unusually large batches.
            scratch.trim(256 * 4 * e);
        });
        if append {
            cache.len = past + t;
        }
        logits
    }
}

/// log-softmax value of `idx` within `logits`.
pub fn log_prob(logits: &[f32], idx: usize) -> f32 {
    let (mx, log_sum) = log_normalizer(logits);
    logits[idx] - mx - log_sum
}

pub(crate) fn log_normalizer(logits: &[f32]) -> (f32, f32) {
    let mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = logits.iter().map(|v| (v - mx).exp()).sum();
    (mx, sum.ln())
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

#[cfg(test)]
mod tests {
    use super::gelu_new;

    #[test]
    fn sigmoid_gelu_matches_tanh_form() {
        let inputs = [
            -12.0f32, -6.0, -3.0, -1.0, -0.1, 0.0, 0.1, 1.0, 3.0, 6.0, 12.0,
        ];
        let mut actual = inputs;
        gelu_new(&mut actual);
        for (x, result) in inputs.into_iter().zip(actual) {
            let expected = 0.5 * x * (1.0 + (0.797_884_6 * (x + 0.044715 * x * x * x)).tanh());
            assert!(
                (result - expected).abs() < 1e-6,
                "{x}: {result} vs {expected}"
            );
        }
    }
}
