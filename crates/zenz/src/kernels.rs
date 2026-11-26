//! Small-batch matmul / dot kernels tuned for IME-sized inputs (a handful of
//! rows): the weight matrix is streamed exactly once, never packed, and
//! column blocks are spread over a small dedicated thread pool.

use std::sync::OnceLock;

pub(crate) fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 8);
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(|i| format!("sweet-zenz-{i}"))
            .build()
            .expect("thread pool")
    })
}

#[derive(Clone, Copy)]
struct SendPtr(*mut f32);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

/// y[m,n] (+)= x[m,k] @ w[k,n]   (w row-major)
pub fn matmul(x: &[f32], w: &[f32], y: &mut [f32], m: usize, k: usize, n: usize, accumulate: bool) {
    matmul_bias(x, w, None, y, m, k, n, accumulate);
}

/// y[m,n] (+)= x[m,k] @ w[k,n] (+ bias[n])   (w row-major)
pub fn matmul_bias(
    x: &[f32],
    w: &[f32],
    bias: Option<&[f32]>,
    y: &mut [f32],
    m: usize,
    k: usize,
    n: usize,
    accumulate: bool,
) {
    assert!(x.len() >= m * k && w.len() >= k * n && y.len() >= m * n);
    if let Some(b) = bias {
        assert!(b.len() >= n);
    }
    // Split columns into chunks (multiples of 16) for the worker threads.
    let work = m * k * n;
    let threads = if work < 1 << 17 {
        1
    } else {
        pool().current_num_threads()
    };
    let chunk = (n.div_ceil(threads) + 15) / 16 * 16;
    let yp = SendPtr(y.as_mut_ptr());
    let run = |j0: usize, j1: usize| {
        let yp = yp;
        // SAFETY: each task writes a disjoint column range [j0, j1) of y.
        unsafe { matmul_cols(x, w, bias, yp.0, m, k, n, j0, j1, accumulate) };
    };
    if threads == 1 {
        run(0, n);
        return;
    }
    pool().scope(|s| {
        let mut j0 = 0;
        while j0 < n {
            let j1 = (j0 + chunk).min(n);
            s.spawn(move |_| run(j0, j1));
            j0 = j1;
        }
    });
}

unsafe fn matmul_cols(
    x: &[f32],
    w: &[f32],
    bias: Option<&[f32]>,
    y: *mut f32,
    m: usize,
    k: usize,
    n: usize,
    j0: usize,
    j1: usize,
    accumulate: bool,
) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
            unsafe { matmul_avx2(x, w, bias, y, m, k, n, j0, j1, accumulate) };
            return;
        }
    }
    // NEON is part of every aarch64 CPU (phones).
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { matmul_neon(x, w, bias, y, m, k, n, j0, j1, accumulate) };
        return;
    }
    #[allow(unreachable_code)]
    unsafe {
        matmul_generic(x, w, bias, y, m, k, n, j0, j1, accumulate)
    };
}

unsafe fn matmul_generic(
    x: &[f32],
    w: &[f32],
    bias: Option<&[f32]>,
    y: *mut f32,
    m: usize,
    k: usize,
    n: usize,
    j0: usize,
    j1: usize,
    accumulate: bool,
) {
    for i in 0..m {
        for jj in j0..j1 {
            let mut acc = bias.map_or(0.0, |b| b[jj]);
            for kk in 0..k {
                acc += x[i * k + kk] * w[kk * n + jj];
            }
            unsafe {
                let dst = y.add(i * n + jj);
                if accumulate {
                    *dst += acc;
                } else {
                    *dst = acc;
                }
            }
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
unsafe fn matmul_avx2(
    x: &[f32],
    w: &[f32],
    bias: Option<&[f32]>,
    y: *mut f32,
    m: usize,
    k: usize,
    n: usize,
    j0: usize,
    j1: usize,
    accumulate: bool,
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    // Register-blocked micro-kernel: R rows x 16 columns of y live in
    // registers while k streams. For multi-block m, pack each KB x 16
    // strip of w into a contiguous L1 buffer once so power-of-2 row
    // strides (e.g. n = 512, 2048) never cause L1 set-conflict misses.
    const KB: usize = 128;
    unsafe {
        let bp = bias.map_or(std::ptr::null(), |b| b.as_ptr());
        let jend = j0 + (j1 - j0) / 16 * 16;
        let mut j = j0;
        #[cfg(target_arch = "x86_64")]
        let pack_threshold = 6;
        #[cfg(target_arch = "x86")]
        let pack_threshold = 2;
        if m > pack_threshold && k > 0 {
            let mut wpack = [0f32; KB * 16];
            while j < jend {
                let mut k0 = 0;
                while k0 < k {
                    let kb = (k - k0).min(KB);
                    let mut wp = w.as_ptr().add(k0 * n + j);
                    let mut pp = wpack.as_mut_ptr();
                    for _ in 0..kb {
                        _mm256_storeu_ps(pp, _mm256_loadu_ps(wp));
                        _mm256_storeu_ps(pp.add(8), _mm256_loadu_ps(wp.add(8)));
                        wp = wp.add(n);
                        pp = pp.add(16);
                    }
                    let first_k = k0 == 0;
                    let load_y = accumulate || !first_k;
                    let bp_k = if first_k { bp } else { std::ptr::null() };
                    let mut i = 0;
                    #[cfg(target_arch = "x86_64")]
                    while i < m {
                        match (m - i, load_y) {
                            (1, false) => micro_packed::<1, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (2, false) => micro_packed::<2, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (3, false) => micro_packed::<3, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (4, false) => micro_packed::<4, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (5, false) => micro_packed::<5, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, false) => micro_packed::<6, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (1, true) => micro_packed::<1, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (2, true) => micro_packed::<2, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (3, true) => micro_packed::<3, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (4, true) => micro_packed::<4, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (5, true) => micro_packed::<5, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, true) => micro_packed::<6, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                        }
                        i += 6;
                    }
                    #[cfg(target_arch = "x86")]
                    while i < m {
                        match (m - i, load_y) {
                            (1, false) => micro_packed::<1, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, false) => micro_packed::<2, false>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (1, true) => micro_packed::<1, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, true) => micro_packed::<2, true>(x, wpack.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                        }
                        i += 2;
                    }
                    k0 += kb;
                }
                j += 16;
            }
        } else {
            while j < jend {
                let mut i = 0;
                #[cfg(target_arch = "x86_64")]
                while i < m {
                    match (m - i, accumulate) {
                        (1, false) => micro::<1, false>(x, w, bp, y, i, j, k, n),
                        (2, false) => micro::<2, false>(x, w, bp, y, i, j, k, n),
                        (3, false) => micro::<3, false>(x, w, bp, y, i, j, k, n),
                        (4, false) => micro::<4, false>(x, w, bp, y, i, j, k, n),
                        (5, false) => micro::<5, false>(x, w, bp, y, i, j, k, n),
                        (_, false) => micro::<6, false>(x, w, bp, y, i, j, k, n),
                        (1, true) => micro::<1, true>(x, w, bp, y, i, j, k, n),
                        (2, true) => micro::<2, true>(x, w, bp, y, i, j, k, n),
                        (3, true) => micro::<3, true>(x, w, bp, y, i, j, k, n),
                        (4, true) => micro::<4, true>(x, w, bp, y, i, j, k, n),
                        (5, true) => micro::<5, true>(x, w, bp, y, i, j, k, n),
                        (_, true) => micro::<6, true>(x, w, bp, y, i, j, k, n),
                    }
                    i += 6;
                }
                #[cfg(target_arch = "x86")]
                while i < m {
                    match (m - i, accumulate) {
                        (1, false) => micro::<1, false>(x, w, bp, y, i, j, k, n),
                        (_, false) => micro::<2, false>(x, w, bp, y, i, j, k, n),
                        (1, true) => micro::<1, true>(x, w, bp, y, i, j, k, n),
                        (_, true) => micro::<2, true>(x, w, bp, y, i, j, k, n),
                    }
                    i += 2;
                }
                j += 16;
            }
        }
        matmul_generic(x, w, bias, y, m, k, n, jend, j1, accumulate);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn micro_packed<const R: usize, const ACCUM: bool>(
    x: &[f32],
    wpack: *const f32,
    bp: *const f32,
    y: *mut f32,
    i: usize,
    j: usize,
    k0: usize,
    kb: usize,
    k: usize,
    n: usize,
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    unsafe {
        let (b0, b1) = if bp.is_null() {
            (_mm256_setzero_ps(), _mm256_setzero_ps())
        } else {
            (_mm256_loadu_ps(bp.add(j)), _mm256_loadu_ps(bp.add(j + 8)))
        };
        let mut acc0 = [b0; R];
        let mut acc1 = [b1; R];
        let xp = x.as_ptr().add(i * k + k0);
        let mut wp = wpack;
        for kk in 0..kb {
            let w0 = _mm256_loadu_ps(wp);
            let w1 = _mm256_loadu_ps(wp.add(8));
            for r in 0..R {
                let a = _mm256_broadcast_ss(&*xp.add(r * k + kk));
                acc0[r] = _mm256_fmadd_ps(a, w0, acc0[r]);
                acc1[r] = _mm256_fmadd_ps(a, w1, acc1[r]);
            }
            wp = wp.add(16);
        }
        for r in 0..R {
            let yp = y.add((i + r) * n + j);
            if ACCUM {
                _mm256_storeu_ps(yp, _mm256_add_ps(_mm256_loadu_ps(yp), acc0[r]));
                _mm256_storeu_ps(yp.add(8), _mm256_add_ps(_mm256_loadu_ps(yp.add(8)), acc1[r]));
            } else {
                _mm256_storeu_ps(yp, acc0[r]);
                _mm256_storeu_ps(yp.add(8), acc1[r]);
            }
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn micro<const R: usize, const ACCUM: bool>(
    x: &[f32],
    w: &[f32],
    bp: *const f32,
    y: *mut f32,
    i: usize,
    j: usize,
    k: usize,
    n: usize,
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    unsafe {
        let (b0, b1) = if bp.is_null() {
            (_mm256_setzero_ps(), _mm256_setzero_ps())
        } else {
            (_mm256_loadu_ps(bp.add(j)), _mm256_loadu_ps(bp.add(j + 8)))
        };
        let mut acc0 = [b0; R];
        let mut acc1 = [b1; R];
        let xp = x.as_ptr().add(i * k);
        let mut wp = w.as_ptr().add(j);
        let pf_stride = n * 4;
        for kk in 0..k {
            if kk + 4 < k {
                _mm_prefetch(wp.add(pf_stride) as *const i8, _MM_HINT_T0);
            }
            let w0 = _mm256_loadu_ps(wp);
            let w1 = _mm256_loadu_ps(wp.add(8));
            for r in 0..R {
                let a = _mm256_broadcast_ss(&*xp.add(r * k + kk));
                acc0[r] = _mm256_fmadd_ps(a, w0, acc0[r]);
                acc1[r] = _mm256_fmadd_ps(a, w1, acc1[r]);
            }
            wp = wp.add(n);
        }
        for r in 0..R {
            let yp = y.add((i + r) * n + j);
            if ACCUM {
                _mm256_storeu_ps(yp, _mm256_add_ps(_mm256_loadu_ps(yp), acc0[r]));
                _mm256_storeu_ps(yp.add(8), _mm256_add_ps(_mm256_loadu_ps(yp.add(8)), acc1[r]));
            } else {
                _mm256_storeu_ps(yp, acc0[r]);
                _mm256_storeu_ps(yp.add(8), acc1[r]);
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn matmul_neon(
    x: &[f32],
    w: &[f32],
    bias: Option<&[f32]>,
    y: *mut f32,
    m: usize,
    k: usize,
    n: usize,
    j0: usize,
    j1: usize,
    accumulate: bool,
) {
    // As matmul_avx2: R rows x 16 columns of y in registers (4 x 4 lanes);
    // aarch64 has 32 vector registers, enough for 6 rows.
    unsafe {
        let bp = bias.map_or(std::ptr::null(), |b| b.as_ptr());
        let jend = j0 + (j1 - j0) / 16 * 16;
        let mut j = j0;
        while j < jend {
            let mut i = 0;
            while i < m {
                match (m - i, accumulate) {
                    (1, false) => micro_neon::<1, false>(x, w, bp, y, i, j, k, n),
                    (2, false) => micro_neon::<2, false>(x, w, bp, y, i, j, k, n),
                    (3, false) => micro_neon::<3, false>(x, w, bp, y, i, j, k, n),
                    (4, false) => micro_neon::<4, false>(x, w, bp, y, i, j, k, n),
                    (5, false) => micro_neon::<5, false>(x, w, bp, y, i, j, k, n),
                    (_, false) => micro_neon::<6, false>(x, w, bp, y, i, j, k, n),
                    (1, true) => micro_neon::<1, true>(x, w, bp, y, i, j, k, n),
                    (2, true) => micro_neon::<2, true>(x, w, bp, y, i, j, k, n),
                    (3, true) => micro_neon::<3, true>(x, w, bp, y, i, j, k, n),
                    (4, true) => micro_neon::<4, true>(x, w, bp, y, i, j, k, n),
                    (5, true) => micro_neon::<5, true>(x, w, bp, y, i, j, k, n),
                    (_, true) => micro_neon::<6, true>(x, w, bp, y, i, j, k, n),
                }
                i += 6;
            }
            j += 16;
        }
        matmul_generic(x, w, bias, y, m, k, n, jend, j1, accumulate);
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn micro_neon<const R: usize, const ACCUM: bool>(
    x: &[f32],
    w: &[f32],
    bp: *const f32,
    y: *mut f32,
    i: usize,
    j: usize,
    k: usize,
    n: usize,
) {
    use std::arch::aarch64::*;
    unsafe {
        let init = if bp.is_null() {
            [vdupq_n_f32(0.0); 4]
        } else {
            let p = bp.add(j);
            [vld1q_f32(p), vld1q_f32(p.add(4)), vld1q_f32(p.add(8)), vld1q_f32(p.add(12))]
        };
        let mut acc = [init; R];
        let xp = x.as_ptr().add(i * k);
        let mut wp = w.as_ptr().add(j);
        for kk in 0..k {
            let wv = [vld1q_f32(wp), vld1q_f32(wp.add(4)), vld1q_f32(wp.add(8)), vld1q_f32(wp.add(12))];
            for r in 0..R {
                let a = vdupq_n_f32(*xp.add(r * k + kk));
                for c in 0..4 {
                    acc[r][c] = vfmaq_f32(acc[r][c], a, wv[c]);
                }
            }
            wp = wp.add(n);
        }
        for r in 0..R {
            let yp = y.add((i + r) * n + j);
            for c in 0..4 {
                let p = yp.add(c * 4);
                if ACCUM {
                    vst1q_f32(p, vaddq_f32(vld1q_f32(p), acc[r][c]));
                } else {
                    vst1q_f32(p, acc[r][c]);
                }
            }
        }
    }
}

/// Vector dot product `sum_i a[i] * b[i]`.
#[inline]
pub(crate) fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: AVX2+FMA checked above and slices have equal length.
        return unsafe { dot_avx2(a, b) };
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `out[i] += w * v[i]`
#[inline]
pub(crate) fn axpy_f32(out: &mut [f32], v: &[f32], w: f32) {
    debug_assert_eq!(out.len(), v.len());
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: AVX2+FMA checked above and slices have equal length.
        unsafe { axpy_avx2(out, v, w) };
        return;
    }
    for (o, &val) in out.iter_mut().zip(v) {
        *o += w * val;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    let len = a.len();
    let mut i = 0;
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();
    unsafe {
        let ap = a.as_ptr();
        let bp = b.as_ptr();
        while i + 16 <= len {
            acc0 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i)), _mm256_loadu_ps(bp.add(i)), acc0);
            acc1 = _mm256_fmadd_ps(
                _mm256_loadu_ps(ap.add(i + 8)),
                _mm256_loadu_ps(bp.add(i + 8)),
                acc1,
            );
            i += 16;
        }
        acc0 = _mm256_add_ps(acc0, acc1);
        while i + 8 <= len {
            acc0 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i)), _mm256_loadu_ps(bp.add(i)), acc0);
            i += 8;
        }
        let hi = _mm256_extractf128_ps(acc0, 1);
        let lo = _mm256_castps256_ps128(acc0);
        let sum128 = _mm_add_ps(lo, hi);
        let shuf = _mm_movehdup_ps(sum128);
        let sums = _mm_add_ps(sum128, shuf);
        let shuf2 = _mm_movehl_ps(sums, sums);
        let mut total = _mm_cvtss_f32(_mm_add_ss(sums, shuf2));
        while i < len {
            total += *ap.add(i) * *bp.add(i);
            i += 1;
        }
        total
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
unsafe fn axpy_avx2(out: &mut [f32], v: &[f32], w: f32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    let len = out.len();
    let mut i = 0;
    let wv = _mm256_set1_ps(w);
    unsafe {
        let op = out.as_mut_ptr();
        let vp = v.as_ptr();
        while i + 16 <= len {
            let o0 = _mm256_loadu_ps(op.add(i));
            let o1 = _mm256_loadu_ps(op.add(i + 8));
            _mm256_storeu_ps(op.add(i), _mm256_fmadd_ps(wv, _mm256_loadu_ps(vp.add(i)), o0));
            _mm256_storeu_ps(
                op.add(i + 8),
                _mm256_fmadd_ps(wv, _mm256_loadu_ps(vp.add(i + 8)), o1),
            );
            i += 16;
        }
        while i + 8 <= len {
            let o0 = _mm256_loadu_ps(op.add(i));
            _mm256_storeu_ps(op.add(i), _mm256_fmadd_ps(wv, _mm256_loadu_ps(vp.add(i)), o0));
            i += 8;
        }
        while i < len {
            *op.add(i) += w * *vp.add(i);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_matches_naive() {
        let (m, k, n) = (7, 37, 300);
        let x: Vec<f32> = (0..m * k).map(|i| ((i * 7 % 13) as f32 - 6.0) / 7.0).collect();
        let w: Vec<f32> = (0..k * n).map(|i| ((i * 11 % 17) as f32 - 8.0) / 9.0).collect();
        let b: Vec<f32> = (0..n).map(|i| ((i * 5 % 11) as f32 - 5.0) / 10.0).collect();
        let mut y = vec![1.0f32; m * n];
        matmul(&x, &w, &mut y, m, k, n, true);
        let mut yb = vec![99.0f32; m * n];
        matmul_bias(&x, &w, Some(&b), &mut yb, m, k, n, false);
        for i in 0..m {
            for j in 0..n {
                let prod: f32 = (0..k).map(|kk| x[i * k + kk] * w[kk * n + j]).sum();
                assert!((y[i * n + j] - (1.0 + prod)).abs() < 1e-3, "{i},{j}");
                assert!((yb[i * n + j] - (b[j] + prod)).abs() < 1e-3, "bias {i},{j}");
            }
        }
        let a: Vec<f32> = (0..64).map(|i| (i as f32 - 32.0) * 0.1).collect();
        let bv: Vec<f32> = (0..64).map(|i| ((i * 3 % 19) as f32 - 9.0) * 0.2).collect();
        let want_dot: f32 = a.iter().zip(&bv).map(|(p, q)| p * q).sum();
        assert!((dot_f32(&a, &bv) - want_dot).abs() < 1e-4);
        let mut out = a.clone();
        axpy_f32(&mut out, &bv, 0.75);
        for i in 0..64 {
            assert!((out[i] - (a[i] + 0.75 * bv[i])).abs() < 1e-5);
        }
    }
}
