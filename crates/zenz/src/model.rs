//! GPT-2 forward pass with a KV cache, loading HF safetensors (F32/BF16/F16).

use std::cell::RefCell;

use crate::kernels::{
    Q8Matrix, attention_row_f32, gelu_f32, layer_norm_f32, log_normalizer_f32, matmul_q8_bias,
};

mod gguf;
mod q8cache;
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
    logits: Vec<f32>,
    anc_data: Vec<usize>,
    anc_ranges: Vec<(usize, usize)>,
}

impl ForwardScratch {
    fn trim(&mut self, max_elems: usize, max_logits: usize) {
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
        if self.logits.capacity() > max_logits {
            self.logits.clear();
            self.logits.shrink_to(max_logits);
        }
        if self.anc_data.capacity() > 4096 {
            self.anc_data.clear();
            self.anc_data.shrink_to(4096);
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
    attn_b: &'a [f32],
    proj_b: &'a [f32],
    ln2_w: &'a [f32],
    ln2_b: &'a [f32],
    fc_b: &'a [f32],
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

struct Q8Layer {
    attn_w: Q8Matrix,
    proj_w: Q8Matrix,
    fc_w: Q8Matrix,
    fc2_w: Q8Matrix,
}

pub struct Model {
    pub cfg: Config,
    store: Store,
    wte: R,   // [V, E]
    wte_t: R, // [E, V] for the LM head
    wte_t_q8: Q8Matrix,
    wpe: R, // [P, E]
    layers: Vec<Layer>,
    q8_layers: Vec<Q8Layer>,
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
    layer_norm_f32(x, w, b, eps, out);
}

fn gelu_new(v: &mut [f32]) {
    if v.len() >= 131_072 {
        use rayon::prelude::*;
        crate::kernels::pool().install(|| v.par_chunks_mut(16_384).for_each(gelu_f32));
    } else {
        gelu_f32(v);
    }
}

impl Model {
    /// Shapes `(k, n)` of the quantized matrices, in the order `quantize`
    /// returns them: the LM head, then the attention, projection and two
    /// feed-forward matrices of each layer.
    fn q8_shapes(cfg: &Config) -> Vec<(usize, usize)> {
        let e = cfg.n_embd;
        let mut shapes = vec![(e, cfg.vocab_size)];
        for _ in 0..cfg.n_layer {
            shapes.extend([(e, 3 * e), (e, e), (e, 4 * e), (4 * e, e)]);
        }
        shapes
    }

    fn quantize(cfg: &Config, d: &[f32], wte_t: &R, layers: &[Layer]) -> Vec<Q8Matrix> {
        let ranges = std::iter::once(wte_t)
            .chain(layers.iter().flat_map(|l| [&l.attn_w, &l.proj_w, &l.fc_w, &l.fc2_w]));
        ranges
            .zip(Self::q8_shapes(cfg))
            .map(|(r, (k, n))| Q8Matrix::from_f32(&d[r.clone()], k, n))
            .collect()
    }

    fn build_q8(
        cfg: Config,
        store: Store,
        wte: R,
        wte_t: R,
        wpe: R,
        layers: Vec<Layer>,
        lnf_w: R,
        lnf_b: R,
        q8: Option<Vec<Q8Matrix>>,
    ) -> Self {
        let q8 = q8.unwrap_or_else(|| Self::quantize(&cfg, store.data(), &wte_t, &layers));
        let mut q8 = q8.into_iter();
        let mut next = || q8.next().expect("one quantized matrix per shape");
        let wte_t_q8 = next();
        let q8_layers: Vec<Q8Layer> = layers
            .iter()
            .map(|_| Q8Layer {
                attn_w: next(),
                proj_w: next(),
                fc_w: next(),
                fc2_w: next(),
            })
            .collect();
        Self {
            cfg,
            store,
            wte,
            wte_t,
            wte_t_q8,
            wpe,
            layers,
            q8_layers,
            lnf_w,
            lnf_b,
        }
    }

    fn view<'a>(&'a self, d: &'a [f32], l: &Layer) -> LayerView<'a> {
        LayerView {
            ln1_w: &d[l.ln1_w.clone()],
            ln1_b: &d[l.ln1_b.clone()],
            attn_b: &d[l.attn_b.clone()],
            proj_b: &d[l.proj_b.clone()],
            ln2_w: &d[l.ln2_w.clone()],
            ln2_b: &d[l.ln2_b.clone()],
            fc_b: &d[l.fc_b.clone()],
            fc2_b: &d[l.fc2_b.clone()],
        }
    }

    pub fn new_cache(&self) -> KvCache {
        KvCache { k: vec![Vec::new(); self.cfg.n_layer], v: vec![Vec::new(); self.cfg.n_layer], len: 0 }
    }

    /// Runs `tokens` after the cached prefix and appends them to the cache.
    /// Returns logits for every position (`want_all`) or only the last one.
    pub fn forward(&self, tokens: &[u32], cache: &mut KvCache, want_all: bool) -> Vec<f32> {
        self.forward_with(tokens, cache, want_all, |logits| logits.to_vec())
    }

    /// Consumes logits directly from the reusable forward scratch buffer.
    pub(crate) fn forward_with<T>(
        &self,
        tokens: &[u32],
        cache: &mut KvCache,
        want_all: bool,
        consume: impl FnOnce(&[f32]) -> T,
    ) -> T {
        self.forward_impl(
            tokens,
            None,
            cache,
            tokens.len(),
            if want_all { 0 } else { tokens.len().saturating_sub(1) },
            consume,
        )
    }

    /// Tree attention: evaluates a whole token trie in one pass. `parents[i]`
    /// is the index (within `tokens`) of node i's parent, or `None` when it
    /// directly follows the cached prefix. Parents must precede children.
    /// The cache is left unchanged. Returns logits `[tokens.len() - logits_from, V]`
    /// for the nodes from index `logits_from` on.
    pub fn forward_tree(&self, tokens: &[u32], parents: &[Option<usize>], cache: &mut KvCache, logits_from: usize) -> Vec<f32> {
        self.forward_tree_with(tokens, parents, cache, logits_from, |logits| logits.to_vec())
    }

    /// Like [`Self::forward_tree`], but borrows the reusable scratch logits
    /// slice directly inside `f` without allocating a fresh `[rows, V]` buffer.
    pub(crate) fn forward_tree_with<T: Send>(
        &self,
        tokens: &[u32],
        parents: &[Option<usize>],
        cache: &mut KvCache,
        logits_from: usize,
        f: impl FnOnce(&[f32]) -> T + Send,
    ) -> T {
        self.forward_tree_caching_with(tokens, parents, cache, 0, logits_from, f)
    }

    /// Like [`Self::forward_tree_with`], and also appends the linear prefix
    /// `tokens[..save_kv_prefix]` to `cache` in the same pass so subsequent
    /// incremental prompts can reuse its K/V states without recomputing them.
    pub(crate) fn forward_tree_caching_with<T: Send>(
        &self,
        tokens: &[u32],
        parents: &[Option<usize>],
        cache: &mut KvCache,
        save_kv_prefix: usize,
        logits_from: usize,
        f: impl FnOnce(&[f32]) -> T + Send,
    ) -> T {
        // Keep the layer work on the same workers as the matrix products.
        crate::kernels::pool().install(|| {
            self.forward_impl(tokens, Some(parents), cache, save_kv_prefix, logits_from, f)
        })
    }

    fn forward_impl<T>(
        &self,
        tokens: &[u32],
        parents: Option<&[Option<usize>]>,
        cache: &mut KvCache,
        save_kv_prefix: usize,
        logits_from: usize,
        consume: impl FnOnce(&[f32]) -> T,
    ) -> T {
        let Config { n_embd: e, n_head: nh, eps, vocab_size: vsz, .. } = self.cfg;
        let t = tokens.len();
        if t == 0 {
            return consume(&[]);
        }
        let past = cache.len;
        let hd = e / nh;
        let scale = 1.0 / (hd as f32).sqrt();
        let d = self.store.data();
        let (wte, wpe) = (&d[self.wte.clone()], &d[self.wpe.clone()]);
        let rows = t - logits_from.min(t - 1);
        // Take scratch out of the TLS cell for the duration of this call so
        // Rayon work-stealing can safely run nested inferences on this thread.
        let mut scratch = FORWARD_SCRATCH.with(|cell| cell.take());
        let out_val = {
            let ForwardScratch {
                x,
                h,
                qkv,
                att,
                ff,
                scores,
                hf,
                logits,
                anc_data,
                anc_ranges,
            } = &mut scratch;
            // Flat ancestor chains (root..=self) within the batch.
            anc_data.clear();
            anc_ranges.clear();
            anc_ranges.reserve(t);
            let mut max_depth = 0usize;
            for i in 0..t {
                let mut start = anc_data.len();
                if let Some(p) = parents.map_or_else(|| i.checked_sub(1), |parents| parents[i]) {
                    assert!(p < i, "parent must precede child");
                    let (ps, pe) = anc_ranges[p];
                    // Extend the previous path in place when its range reaches
                    // the end of the buffer. Earlier ranges remain valid.
                    if pe == start {
                        start = ps;
                    } else {
                        anc_data.extend_from_within(ps..pe);
                    }
                }
                anc_data.push(i);
                let end = anc_data.len();
                max_depth = max_depth.max(end - start);
                anc_ranges.push((start, end));
            }
            let max_ctx = past + max_depth;
            x.resize(t * e, 0.0);
            h.resize(t * e, 0.0);
            qkv.resize(t * 3 * e, 0.0);
            att.resize(t * e, 0.0);
            ff.resize(t * 4 * e, 0.0);
            scores.resize(max_ctx, 0.0);
            hf.resize(rows * e, 0.0);
            logits.resize(rows * vsz, 0.0);
            for (i, &tok) in tokens.iter().enumerate() {
                let (as_, ae) = anc_ranges[i];
                let pos = (past + (ae - as_) - 1).min(self.cfg.n_positions - 1);
                let te = &wte[tok as usize * e..][..e];
                let pe = &wpe[pos * e..][..e];
                let xr = &mut x[i * e..][..e];
                for j in 0..e {
                    xr[j] = te[j] + pe[j];
                }
            }
            for (li, (lr, ql)) in self.layers.iter().zip(&self.q8_layers).enumerate() {
                let l = self.view(d, lr);
                layer_norm(x, l.ln1_w, l.ln1_b, eps, h);
                matmul_q8_bias(h, &ql.attn_w, Some(l.attn_b), qkv, t, false);
                // All rows supply keys and values. In the final layer, only
                // the rows whose logits are requested need attention output
                // and the feed-forward network.
                let start = if li + 1 == self.layers.len() {
                    t - rows
                } else {
                    0
                };
                let active = t - start;
                let att = &mut att[..active * e];
                {
                    let kc = &cache.k[li];
                    let vc = &cache.v[li];
                    let anc_data_ref: &[usize] = anc_data;
                    let anc_ranges_ref: &[(usize, usize)] = anc_ranges;
                    let run_row = |i: usize, att_row: &mut [f32], row_scores: &mut [f32]| {
                        let (as_, ae) = anc_ranges_ref[i];
                        attention_row_f32(
                            qkv,
                            kc,
                            vc,
                            &anc_data_ref[as_..ae],
                            past,
                            e,
                            nh,
                            hd,
                            scale,
                            i,
                            att_row,
                            row_scores,
                        );
                    };
                    if active >= 16 {
                        use rayon::prelude::*;
                        att.par_chunks_exact_mut(e)
                            .enumerate()
                            .for_each_init(
                                || vec![0f32; max_ctx],
                                |row_scores, (i, att_row)| run_row(start + i, att_row, row_scores),
                            );
                    } else {
                        for (i, att_row) in att.chunks_exact_mut(e).enumerate() {
                            run_row(start + i, att_row, scores);
                        }
                    }
                }
                let save_n = save_kv_prefix.min(t);
                if save_n > 0 {
                    for i in 0..save_n {
                        cache.k[li].extend_from_slice(&qkv[i * 3 * e + e..i * 3 * e + 2 * e]);
                        cache.v[li].extend_from_slice(&qkv[i * 3 * e + 2 * e..i * 3 * e + 3 * e]);
                    }
                }
                let x = &mut x[start * e..];
                let h = &mut h[..active * e];
                let ff = &mut ff[..active * 4 * e];
                matmul_q8_bias(att, &ql.proj_w, Some(l.proj_b), x, active, true);
                layer_norm(x, l.ln2_w, l.ln2_b, eps, h);
                matmul_q8_bias(h, &ql.fc_w, Some(l.fc_b), ff, active, false);
                gelu_new(ff);
                matmul_q8_bias(ff, &ql.fc2_w, Some(l.fc2_b), x, active, true);
            }
            let src = &x[(t - rows) * e..];
            layer_norm(src, &d[self.lnf_w.clone()], &d[self.lnf_b.clone()], eps, hf);
            matmul_q8_bias(hf, &self.wte_t_q8, None, logits, rows, false);
            consume(&logits[..rows * vsz])
        };
        // Keep steady-state memory bounded after unusually large batches.
        scratch.trim(256 * 4 * e, 64 * vsz);
        FORWARD_SCRATCH.with(|cell| {
            *cell.borrow_mut() = scratch;
        });
        let save_n = save_kv_prefix.min(t);
        if save_n > 0 {
            cache.len = past + save_n;
        }
        out_val
    }
}

/// log-softmax value of `idx` within `logits`.
pub fn log_prob(logits: &[f32], idx: usize) -> f32 {
    let (mx, log_sum) = log_normalizer(logits);
    logits[idx] - mx - log_sum
}

pub(crate) fn log_normalizer(logits: &[f32]) -> (f32, f32) {
    log_normalizer_f32(logits)
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
