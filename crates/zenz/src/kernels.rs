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
        pool().current_num_threads().min((n / 32).max(1))
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
    const KB: usize = 256;
    #[repr(align(32))]
    struct AlignedPack([f32; KB * 16]);
    unsafe {
        let bp = bias.map_or(std::ptr::null(), |b| b.as_ptr());
        let jend = j0 + (j1 - j0) / 16 * 16;
        let mut j = j0;
        #[cfg(target_arch = "x86_64")]
        let pack_threshold = 6;
        #[cfg(target_arch = "x86")]
        let pack_threshold = 2;
        if m > pack_threshold && k > 0 {
            let mut wpack = AlignedPack([0f32; KB * 16]);
            let mut k0 = 0;
            while k0 < k {
                let kb = (k - k0).min(KB);
                let first_k = k0 == 0;
                let load_y = accumulate || !first_k;
                let bp_k = if first_k { bp } else { std::ptr::null() };
                j = j0;
                while j < jend {
                    let mut wp = w.as_ptr().add(k0 * n + j);
                    let mut pp = wpack.0.as_mut_ptr();
                    let pf_stride = n * 4;
                    for kk in 0..kb {
                        if kk + 4 < kb {
                            _mm_prefetch(wp.add(pf_stride) as *const i8, _MM_HINT_T0);
                        }
                        _mm256_store_ps(pp, _mm256_loadu_ps(wp));
                        _mm256_store_ps(pp.add(8), _mm256_loadu_ps(wp.add(8)));
                        wp = wp.add(n);
                        pp = pp.add(16);
                    }
                    let mut i = 0;
                    #[cfg(target_arch = "x86_64")]
                    while i < m {
                        match (m - i, load_y) {
                            (1, false) => micro_packed::<1, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (2, false) => micro_packed::<2, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (3, false) => micro_packed::<3, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (4, false) => micro_packed::<4, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (5, false) => micro_packed::<5, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, false) => micro_packed::<6, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (1, true) => micro_packed::<1, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (2, true) => micro_packed::<2, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (3, true) => micro_packed::<3, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (4, true) => micro_packed::<4, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (5, true) => micro_packed::<5, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, true) => micro_packed::<6, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                        }
                        i += 6;
                    }
                    #[cfg(target_arch = "x86")]
                    while i < m {
                        match (m - i, load_y) {
                            (1, false) => micro_packed::<1, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, false) => micro_packed::<2, false>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (1, true) => micro_packed::<1, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                            (_, true) => micro_packed::<2, true>(x, wpack.0.as_ptr(), bp_k, y, i, j, k0, kb, k, n),
                        }
                        i += 2;
                    }
                    j += 16;
                }
                k0 += kb;
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
        let mut kk = 0;
        while kk + 2 <= kb {
            let w0a = _mm256_load_ps(wp);
            let w1a = _mm256_load_ps(wp.add(8));
            let w0b = _mm256_load_ps(wp.add(16));
            let w1b = _mm256_load_ps(wp.add(24));
            for r in 0..R {
                let aa = _mm256_broadcast_ss(&*xp.add(r * k + kk));
                acc0[r] = _mm256_fmadd_ps(aa, w0a, acc0[r]);
                acc1[r] = _mm256_fmadd_ps(aa, w1a, acc1[r]);
                let ab = _mm256_broadcast_ss(&*xp.add(r * k + kk + 1));
                acc0[r] = _mm256_fmadd_ps(ab, w0b, acc0[r]);
                acc1[r] = _mm256_fmadd_ps(ab, w1b, acc1[r]);
            }
            wp = wp.add(32);
            kk += 2;
        }
        while kk < kb {
            let w0 = _mm256_load_ps(wp);
            let w1 = _mm256_load_ps(wp.add(8));
            for r in 0..R {
                let a = _mm256_broadcast_ss(&*xp.add(r * k + kk));
                acc0[r] = _mm256_fmadd_ps(a, w0, acc0[r]);
                acc1[r] = _mm256_fmadd_ps(a, w1, acc1[r]);
            }
            wp = wp.add(16);
            kk += 1;
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

/// Fast vectorized `exp(x)` for 8 f32 lanes using Cody-Waite range reduction
/// and a degree-5 minimax polynomial. Accurate to `< 1e-6` relative error.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn exp_ps_avx2(x: std::arch::x86_64::__m256) -> std::arch::x86_64::__m256 {
    use std::arch::x86_64::*;
    let x = _mm256_max_ps(_mm256_min_ps(x, _mm256_set1_ps(88.376_26)), _mm256_set1_ps(-88.376_26));
    let log2e = _mm256_set1_ps(std::f32::consts::LOG2_E);
    let fx = _mm256_round_ps(_mm256_mul_ps(x, log2e), _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC);
    // Cody-Waite range reduction: r = x - fx * ln2_hi - fx * ln2_lo
    let ln2_hi = _mm256_set1_ps(0.693_359_4);
    let ln2_lo = _mm256_set1_ps(-2.121_944_4e-4);
    let r = _mm256_fnmadd_ps(fx, ln2_lo, _mm256_fnmadd_ps(fx, ln2_hi, x));
    let mut p = _mm256_set1_ps(1.388_888_9e-3);
    p = _mm256_fmadd_ps(p, r, _mm256_set1_ps(8.333_334e-3));
    p = _mm256_fmadd_ps(p, r, _mm256_set1_ps(4.166_666_8e-2));
    p = _mm256_fmadd_ps(p, r, _mm256_set1_ps(1.666_666_7e-1));
    p = _mm256_fmadd_ps(p, r, _mm256_set1_ps(0.5));
    p = _mm256_fmadd_ps(p, _mm256_mul_ps(r, r), _mm256_add_ps(r, _mm256_set1_ps(1.0)));
    let k = _mm256_cvtps_epi32(fx);
    let pow2k = _mm256_castsi256_ps(_mm256_slli_epi32(_mm256_add_epi32(k, _mm256_set1_epi32(127)), 23));
    _mm256_mul_ps(p, pow2k)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn hsum256_ps(v: std::arch::x86_64::__m256) -> f32 {
    use std::arch::x86_64::*;
    let hi = _mm256_extractf128_ps(v, 1);
    let lo = _mm256_castps256_ps128(v);
    let sum128 = _mm_add_ps(lo, hi);
    let shuf = _mm_movehdup_ps(sum128);
    let sums = _mm_add_ps(sum128, shuf);
    let shuf2 = _mm_movehl_ps(sums, sums);
    _mm_cvtss_f32(_mm_add_ss(sums, shuf2))
}

/// In-place GPT-2 GELU (`x / (1 + exp(-2 * sqrt(2/pi) * (x + 0.044715 * x^3)))`).
pub(crate) fn gelu_f32(v: &mut [f32]) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        unsafe { gelu_avx2(v) };
        return;
    }
    const C: f32 = -1.595_769_2;
    for x in v {
        let exponent = C * (*x + 0.044715 * *x * *x * *x);
        *x /= 1.0 + exponent.exp();
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn gelu_avx2(v: &mut [f32]) {
    use std::arch::x86_64::*;
    const C: f32 = -1.595_769_2;
    let c_vec = _mm256_set1_ps(C);
    let k_vec = _mm256_set1_ps(0.044715);
    let one = _mm256_set1_ps(1.0);
    let len = v.len();
    let ptr = v.as_mut_ptr();
    let mut i = 0;
    unsafe {
        while i + 8 <= len {
            let x = _mm256_loadu_ps(ptr.add(i));
            let x2 = _mm256_mul_ps(x, x);
            let x3 = _mm256_mul_ps(x2, x);
            let inner = _mm256_fmadd_ps(k_vec, x3, x);
            let exp_val = exp_ps_avx2(_mm256_mul_ps(c_vec, inner));
            let res = _mm256_div_ps(x, _mm256_add_ps(one, exp_val));
            _mm256_storeu_ps(ptr.add(i), res);
            i += 8;
        }
        while i < len {
            let x = *ptr.add(i);
            let exponent = C * (x + 0.044715 * x * x * x);
            *ptr.add(i) = x / (1.0 + exponent.exp());
            i += 1;
        }
    }
}

/// Returns `(max(logits), ln(sum(exp(logits - max))))`.
pub(crate) fn log_normalizer_f32(logits: &[f32]) -> (f32, f32) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        return unsafe { log_normalizer_avx2(logits) };
    }
    let mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = logits.iter().map(|v| (v - mx).exp()).sum();
    (mx, sum.ln())
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn log_normalizer_avx2(logits: &[f32]) -> (f32, f32) {
    use std::arch::x86_64::*;
    let len = logits.len();
    let ptr = logits.as_ptr();
    let mut i = 0;
    let mut vmax0 = _mm256_set1_ps(f32::NEG_INFINITY);
    let mut vmax1 = _mm256_set1_ps(f32::NEG_INFINITY);
    unsafe {
        while i + 16 <= len {
            vmax0 = _mm256_max_ps(vmax0, _mm256_loadu_ps(ptr.add(i)));
            vmax1 = _mm256_max_ps(vmax1, _mm256_loadu_ps(ptr.add(i + 8)));
            i += 16;
        }
        vmax0 = _mm256_max_ps(vmax0, vmax1);
        while i + 8 <= len {
            vmax0 = _mm256_max_ps(vmax0, _mm256_loadu_ps(ptr.add(i)));
            i += 8;
        }
        let hi = _mm256_extractf128_ps(vmax0, 1);
        let lo = _mm256_castps256_ps128(vmax0);
        let m128 = _mm_max_ps(lo, hi);
        let m64 = _mm_max_ps(m128, _mm_movehl_ps(m128, m128));
        let m32 = _mm_max_ss(m64, _mm_shuffle_ps(m64, m64, 1));
        let mut mx = _mm_cvtss_f32(m32);
        while i < len {
            mx = mx.max(*ptr.add(i));
            i += 1;
        }

        let vmx = _mm256_set1_ps(mx);
        let mut sum0 = _mm256_setzero_ps();
        let mut sum1 = _mm256_setzero_ps();
        let mut j = 0;
        while j + 16 <= len {
            let e0 = exp_ps_avx2(_mm256_sub_ps(_mm256_loadu_ps(ptr.add(j)), vmx));
            let e1 = exp_ps_avx2(_mm256_sub_ps(_mm256_loadu_ps(ptr.add(j + 8)), vmx));
            sum0 = _mm256_add_ps(sum0, e0);
            sum1 = _mm256_add_ps(sum1, e1);
            j += 16;
        }
        sum0 = _mm256_add_ps(sum0, sum1);
        while j + 8 <= len {
            let e0 = exp_ps_avx2(_mm256_sub_ps(_mm256_loadu_ps(ptr.add(j)), vmx));
            sum0 = _mm256_add_ps(sum0, e0);
            j += 8;
        }
        let mut sum = hsum256_ps(sum0);
        while j < len {
            sum += (*ptr.add(j) - mx).exp();
            j += 1;
        }
        (mx, sum.ln())
    }
}

/// Row-wise LayerNorm across `x` (`[rows, e]`) into `out`.
pub(crate) fn layer_norm_f32(x: &[f32], w: &[f32], b: &[f32], eps: f32, out: &mut [f32]) {
    assert!(b.len() >= w.len());
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        unsafe { layer_norm_avx2(x, w, b, eps, out) };
        return;
    }
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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn layer_norm_avx2(x: &[f32], w: &[f32], b: &[f32], eps: f32, out: &mut [f32]) {
    use std::arch::x86_64::*;
    let e = w.len();
    let inv_e = 1.0 / e as f32;
    let wp = w.as_ptr();
    let bp = b.as_ptr();
    for (xr, or) in x.chunks_exact(e).zip(out.chunks_exact_mut(e)) {
        unsafe {
            let xp = xr.as_ptr();
            let op = or.as_mut_ptr();
            let mut s0 = _mm256_setzero_ps();
            let mut s1 = _mm256_setzero_ps();
            let mut i = 0;
            while i + 16 <= e {
                s0 = _mm256_add_ps(s0, _mm256_loadu_ps(xp.add(i)));
                s1 = _mm256_add_ps(s1, _mm256_loadu_ps(xp.add(i + 8)));
                i += 16;
            }
            s0 = _mm256_add_ps(s0, s1);
            while i + 8 <= e {
                s0 = _mm256_add_ps(s0, _mm256_loadu_ps(xp.add(i)));
                i += 8;
            }
            let mut sum = hsum256_ps(s0);
            while i < e {
                sum += *xp.add(i);
                i += 1;
            }
            let mean = sum * inv_e;
            let vmean = _mm256_set1_ps(mean);
            let mut v0 = _mm256_setzero_ps();
            let mut v1 = _mm256_setzero_ps();
            let mut i = 0;
            while i + 16 <= e {
                let d0 = _mm256_sub_ps(_mm256_loadu_ps(xp.add(i)), vmean);
                let d1 = _mm256_sub_ps(_mm256_loadu_ps(xp.add(i + 8)), vmean);
                v0 = _mm256_fmadd_ps(d0, d0, v0);
                v1 = _mm256_fmadd_ps(d1, d1, v1);
                i += 16;
            }
            v0 = _mm256_add_ps(v0, v1);
            while i + 8 <= e {
                let d0 = _mm256_sub_ps(_mm256_loadu_ps(xp.add(i)), vmean);
                v0 = _mm256_fmadd_ps(d0, d0, v0);
                i += 8;
            }
            let mut var_sum = hsum256_ps(v0);
            while i < e {
                let d = *xp.add(i) - mean;
                var_sum += d * d;
                i += 1;
            }
            let inv = 1.0 / (var_sum * inv_e + eps).sqrt();
            let vinv = _mm256_set1_ps(inv);
            let mut i = 0;
            while i + 8 <= e {
                let d = _mm256_mul_ps(_mm256_sub_ps(_mm256_loadu_ps(xp.add(i)), vmean), vinv);
                let y = _mm256_fmadd_ps(d, _mm256_loadu_ps(wp.add(i)), _mm256_loadu_ps(bp.add(i)));
                _mm256_storeu_ps(op.add(i), y);
                i += 8;
            }
            while i < e {
                *op.add(i) = (*xp.add(i) - mean) * inv * *wp.add(i) + *bp.add(i);
                i += 1;
            }
        }
    }
}

/// Computes multi-head attention for a single token row `i`, dispatching SIMD
/// once per row instead of on every head/key dot product.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attention_row_f32(
    qkv: &[f32],
    kc: &[f32],
    vc: &[f32],
    anc_i: &[usize],
    past: usize,
    e: usize,
    nh: usize,
    hd: usize,
    scale: f32,
    i: usize,
    att_row: &mut [f32],
    row_scores: &mut [f32],
) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        unsafe {
            attention_row_avx2(qkv, kc, vc, anc_i, past, e, nh, hd, scale, i, att_row, row_scores)
        };
        return;
    }
    let n_ctx = past + anc_i.len();
    for hh in 0..nh {
        let off = hh * hd;
        let q = &qkv[i * 3 * e + off..][..hd];
        let mut mx = f32::NEG_INFINITY;
        for j in 0..n_ctx {
            let k: &[f32] = if j < past {
                &kc[j * e + off..][..hd]
            } else {
                &qkv[anc_i[j - past] * 3 * e + e + off..][..hd]
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
                &qkv[anc_i[j - past] * 3 * e + 2 * e + off..][..hd]
            };
            axpy_f32(out, v, w);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
unsafe fn attention_row_avx2(
    qkv: &[f32],
    kc: &[f32],
    vc: &[f32],
    anc_i: &[usize],
    past: usize,
    e: usize,
    nh: usize,
    hd: usize,
    scale: f32,
    i: usize,
    att_row: &mut [f32],
    row_scores: &mut [f32],
) {
    use std::arch::x86_64::*;
    let n_ctx = past + anc_i.len();
    let q_base = i * 3 * e;
    for hh in 0..nh {
        let off = hh * hd;
        let q = &qkv[q_base + off..][..hd];
        let mut mx = f32::NEG_INFINITY;
        for j in 0..n_ctx {
            let k: &[f32] = if j < past {
                &kc[j * e + off..][..hd]
            } else {
                &qkv[anc_i[j - past] * 3 * e + e + off..][..hd]
            };
            let s = unsafe { dot_avx2(q, k) } * scale;
            row_scores[j] = s;
            mx = mx.max(s);
        }
        let vmx = _mm256_set1_ps(mx);
        let mut sum_vec = _mm256_setzero_ps();
        let sp = row_scores.as_mut_ptr();
        let mut j = 0;
        unsafe {
            while j + 8 <= n_ctx {
                let ev = exp_ps_avx2(_mm256_sub_ps(_mm256_loadu_ps(sp.add(j)), vmx));
                _mm256_storeu_ps(sp.add(j), ev);
                sum_vec = _mm256_add_ps(sum_vec, ev);
                j += 8;
            }
            let mut sum = hsum256_ps(sum_vec);
            while j < n_ctx {
                let ev = (*sp.add(j) - mx).exp();
                *sp.add(j) = ev;
                sum += ev;
                j += 1;
            }
            let out = &mut att_row[off..][..hd];
            out.fill(0.0);
            let inv_sum = sum.recip();
            for j in 0..n_ctx {
                let w = *sp.add(j) * inv_sum;
                let v: &[f32] = if j < past {
                    &vc[j * e + off..][..hd]
                } else {
                    &qkv[anc_i[j - past] * 3 * e + 2 * e + off..][..hd]
                };
                axpy_avx2(out, v, w);
            }
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

        // Verify SIMD GELU, log_normalizer, and layer_norm against scalar reference.
        let mut g: Vec<f32> = (-20..=20).map(|i| i as f32 * 0.4).collect();
        let g_orig = g.clone();
        gelu_f32(&mut g);
        for (&x, &y) in g_orig.iter().zip(&g) {
            let expected = 0.5 * x * (1.0 + (0.797_884_6 * (x + 0.044715 * x * x * x)).tanh());
            assert!((y - expected).abs() < 2e-5, "gelu({x}): {y} vs {expected}");
        }

        let logits: Vec<f32> = (0..137).map(|i| ((i * 13 % 29) as f32 - 14.0) * 0.35).collect();
        let (mx, lse) = log_normalizer_f32(&logits);
        let ref_mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let ref_lse: f32 = logits.iter().map(|v| (v - ref_mx).exp()).sum::<f32>().ln();
        assert!((mx - ref_mx).abs() < 1e-6);
        assert!((lse - ref_lse).abs() < 2e-5, "lse: {lse} vs {ref_lse}");

        let w_ln: Vec<f32> = (0..67).map(|i| 0.5 + (i % 7) as f32 * 0.1).collect();
        let b_ln: Vec<f32> = (0..67).map(|i| -0.3 + (i % 5) as f32 * 0.15).collect();
        let x_ln: Vec<f32> = (0..67 * 2).map(|i| ((i * 7 % 23) as f32 - 11.0) * 0.25).collect();
        let mut out_ln = vec![0f32; x_ln.len()];
        layer_norm_f32(&x_ln, &w_ln, &b_ln, 1e-5, &mut out_ln);
        for row in 0..2 {
            let xr = &x_ln[row * 67..(row + 1) * 67];
            let or = &out_ln[row * 67..(row + 1) * 67];
            let mean = xr.iter().sum::<f32>() / 67.0;
            let var = xr.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / 67.0;
            let inv = 1.0 / (var + 1e-5).sqrt();
            for i in 0..67 {
                let want = (xr[i] - mean) * inv * w_ln[i] + b_ln[i];
                assert!((or[i] - want).abs() < 1e-4, "ln[{row},{i}]: {} vs {want}", or[i]);
            }
        }
    }
}
