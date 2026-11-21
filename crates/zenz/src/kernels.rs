//! Small-batch matmul / dot kernels tuned for IME-sized inputs (a handful of
//! rows): the weight matrix is streamed exactly once, never packed, and
//! column blocks are spread over a small dedicated thread pool.

use std::sync::OnceLock;

fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 4);
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
    assert!(x.len() >= m * k && w.len() >= k * n && y.len() >= m * n);
    if !accumulate {
        y[..m * n].fill(0.0);
    }
    // Split columns into chunks (multiples of 16) for the worker threads.
    let work = m * k * n;
    let threads = if work < 1 << 18 { 1 } else { pool().current_num_threads() };
    let chunk = (n.div_ceil(threads) + 15) / 16 * 16;
    let yp = SendPtr(y.as_mut_ptr());
    let run = |j0: usize, j1: usize| {
        let yp = yp;
        // SAFETY: each task writes a disjoint column range [j0, j1) of y.
        unsafe { matmul_cols(x, w, yp.0, m, k, n, j0, j1) };
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

unsafe fn matmul_cols(x: &[f32], w: &[f32], y: *mut f32, m: usize, k: usize, n: usize, j0: usize, j1: usize) {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
            unsafe { matmul_avx2(x, w, y, m, k, n, j0, j1) };
            return;
        }
    }
    unsafe { matmul_generic(x, w, y, m, k, n, j0, j1) };
}

unsafe fn matmul_generic(x: &[f32], w: &[f32], y: *mut f32, m: usize, k: usize, n: usize, j0: usize, j1: usize) {
    for i in 0..m {
        for jj in j0..j1 {
            let mut acc = 0.0;
            for kk in 0..k {
                acc += x[i * k + kk] * w[kk * n + jj];
            }
            unsafe { *y.add(i * n + jj) += acc };
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn matmul_avx2(x: &[f32], w: &[f32], y: *mut f32, m: usize, k: usize, n: usize, j0: usize, j1: usize) {
    // Register-blocked micro-kernel: R rows x 16 columns of y live in
    // registers while k streams. Column blocks are the outer loop so the
    // k x 16 slice of w stays in cache across all row blocks.
    unsafe {
        let jend = j0 + (j1 - j0) / 16 * 16;
        let mut j = j0;
        while j < jend {
            let mut i = 0;
            while i < m {
                match m - i {
                    1 => micro::<1>(x, w, y, i, j, k, n),
                    2 => micro::<2>(x, w, y, i, j, k, n),
                    3 => micro::<3>(x, w, y, i, j, k, n),
                    4 => micro::<4>(x, w, y, i, j, k, n),
                    5 => micro::<5>(x, w, y, i, j, k, n),
                    _ => micro::<6>(x, w, y, i, j, k, n),
                }
                i += 6;
            }
            j += 16;
        }
        matmul_generic(x, w, y, m, k, n, jend, j1);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn micro<const R: usize>(x: &[f32], w: &[f32], y: *mut f32, i: usize, j: usize, k: usize, n: usize) {
    use std::arch::x86_64::*;
    unsafe {
        let mut acc0 = [_mm256_setzero_ps(); R];
        let mut acc1 = [_mm256_setzero_ps(); R];
        let xp = x.as_ptr().add(i * k);
        let mut wp = w.as_ptr().add(j);
        for kk in 0..k {
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
            _mm256_storeu_ps(yp, _mm256_add_ps(_mm256_loadu_ps(yp), acc0[r]));
            _mm256_storeu_ps(yp.add(8), _mm256_add_ps(_mm256_loadu_ps(yp.add(8)), acc1[r]));
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
        let mut y = vec![1.0f32; m * n];
        matmul(&x, &w, &mut y, m, k, n, true);
        for i in 0..m {
            for j in 0..n {
                let want: f32 = 1.0 + (0..k).map(|kk| x[i * k + kk] * w[kk * n + j]).sum::<f32>();
                assert!((y[i * n + j] - want).abs() < 1e-3, "{i},{j}");
            }
        }
    }
}
