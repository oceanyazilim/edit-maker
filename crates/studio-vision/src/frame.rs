//! Per-frame feature extraction on small RGB24 frames.

pub const GRID_W: usize = 16;
pub const GRID_H: usize = 9;
pub const HIST_BINS: usize = 16;

#[derive(Clone)]
pub struct FrameFeat {
    pub luma: Vec<f32>,
    pub hist: [f32; HIST_BINS * 3],
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub sharpness: f32,
    /// Per-cell detail (gradient energy) on a GRID_W x GRID_H grid.
    pub detail_grid: [f32; GRID_W * GRID_H],
}

pub fn extract(rgb: &[u8], w: usize, h: usize) -> FrameFeat {
    let n = w * h;
    let mut luma = vec![0f32; n];
    let mut hist = [0f32; HIST_BINS * 3];
    let mut sat_sum = 0f32;
    for i in 0..n {
        let r = rgb[i * 3] as f32 / 255.0;
        let g = rgb[i * 3 + 1] as f32 / 255.0;
        let b = rgb[i * 3 + 2] as f32 / 255.0;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        luma[i] = y;
        // Histogram in a decorrelated space: Y, (R-G), (B - (R+G)/2).
        let c1 = ((r - g) * 0.5 + 0.5).clamp(0.0, 0.9999);
        let c2 = ((b - (r + g) * 0.5) * 0.5 + 0.5).clamp(0.0, 0.9999);
        hist[(y.clamp(0.0, 0.9999) * HIST_BINS as f32) as usize] += 1.0;
        hist[HIST_BINS + (c1 * HIST_BINS as f32) as usize] += 1.0;
        hist[2 * HIST_BINS + (c2 * HIST_BINS as f32) as usize] += 1.0;
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        sat_sum += if mx > 1e-3 { (mx - mn) / mx } else { 0.0 };
    }
    for v in hist.iter_mut() {
        *v /= n as f32;
    }
    let brightness = luma.iter().sum::<f32>() / n as f32;
    let contrast = (luma
        .iter()
        .map(|y| (y - brightness) * (y - brightness))
        .sum::<f32>()
        / n as f32)
        .sqrt();

    // Laplacian energy (sharpness) + gradient detail grid.
    let mut lap_sum = 0f32;
    let mut grid = [0f32; GRID_W * GRID_H];
    let mut grid_n = [0f32; GRID_W * GRID_H];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let c = luma[y * w + x];
            let lap = 4.0 * c
                - luma[y * w + x - 1]
                - luma[y * w + x + 1]
                - luma[(y - 1) * w + x]
                - luma[(y + 1) * w + x];
            lap_sum += lap * lap;
            let gx = luma[y * w + x + 1] - luma[y * w + x - 1];
            let gy = luma[(y + 1) * w + x] - luma[(y - 1) * w + x];
            let gi = (y * GRID_H / h).min(GRID_H - 1) * GRID_W + (x * GRID_W / w).min(GRID_W - 1);
            grid[gi] += (gx * gx + gy * gy).sqrt();
            grid_n[gi] += 1.0;
        }
    }
    for i in 0..grid.len() {
        if grid_n[i] > 0.0 {
            grid[i] /= grid_n[i];
        }
    }
    let inner = ((w.saturating_sub(2)) * (h.saturating_sub(2))).max(1) as f32;
    FrameFeat {
        luma,
        hist,
        brightness,
        contrast,
        saturation: sat_sum / n as f32,
        sharpness: lap_sum / inner,
        detail_grid: grid,
    }
}

/// 0..1 histogram distance (1 - intersection, averaged over the three channels).
pub fn hist_distance(a: &[f32; HIST_BINS * 3], b: &[f32; HIST_BINS * 3]) -> f32 {
    let inter: f32 = a.iter().zip(b.iter()).map(|(x, y)| x.min(*y)).sum();
    (1.0 - inter / 3.0).clamp(0.0, 1.0)
}

/// Global motion by exhaustive block matching of a downsampled luma (±`r` px).
/// Returns (dx, dy, residual mean abs diff after compensation, raw mean abs diff).
pub fn global_motion(
    prev: &[f32],
    cur: &[f32],
    w: usize,
    h: usize,
    r: i32,
) -> (f32, f32, f32, f32) {
    let margin = r as usize;
    if w <= 2 * margin + 2 || h <= 2 * margin + 2 {
        let raw = mean_abs_diff(prev, cur);
        return (0.0, 0.0, raw, raw);
    }
    let sad = |dx: i32, dy: i32| -> f32 {
        let mut s = 0f32;
        let mut n = 0u32;
        for y in margin..h - margin {
            let yy = (y as i32 + dy) as usize;
            for x in (margin..w - margin).step_by(1) {
                let xx = (x as i32 + dx) as usize;
                s += (cur[y * w + x] - prev[yy * w + xx]).abs();
                n += 1;
            }
        }
        s / n.max(1) as f32
    };
    let raw = sad(0, 0);
    let (mut best, mut bdx, mut bdy) = (raw, 0, 0);
    for dy in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dy == 0 {
                continue;
            }
            let s = sad(dx, dy);
            // Small bias towards zero motion to avoid noise-induced drift.
            if s < best * 0.97 {
                best = s;
                bdx = dx;
                bdy = dy;
            }
        }
    }
    // Content moved by -d (we matched cur(x) with prev(x + d)), so camera/content motion is -d.
    (-(bdx as f32), -(bdy as f32), best, raw)
}

pub fn mean_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() {
        return 0.0;
    }
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .sum::<f32>()
        / a.len() as f32
}

/// Box downsample of a luma plane by integer factor.
pub fn downsample(l: &[f32], w: usize, h: usize, f: usize) -> (Vec<f32>, usize, usize) {
    let (nw, nh) = (w / f, h / f);
    let mut out = vec![0f32; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let mut s = 0f32;
            for yy in 0..f {
                for xx in 0..f {
                    s += l[(y * f + yy) * w + x * f + xx];
                }
            }
            out[y * nw + x] = s / (f * f) as f32;
        }
    }
    (out, nw, nh)
}

/// Per-cell mean absolute difference on the analysis grid.
pub fn diff_grid(a: &[f32], b: &[f32], w: usize, h: usize) -> [f32; GRID_W * GRID_H] {
    let mut g = [0f32; GRID_W * GRID_H];
    let mut n = [0f32; GRID_W * GRID_H];
    for y in 0..h {
        for x in 0..w {
            let gi = (y * GRID_H / h).min(GRID_H - 1) * GRID_W + (x * GRID_W / w).min(GRID_W - 1);
            g[gi] += (a[y * w + x] - b[y * w + x]).abs();
            n[gi] += 1.0;
        }
    }
    for i in 0..g.len() {
        if n[i] > 0.0 {
            g[i] /= n[i];
        }
    }
    g
}
