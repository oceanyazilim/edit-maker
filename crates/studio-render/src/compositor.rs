//! CPU reference compositor (RGBA8, premultiplied-free, sRGB-space operations).
//! Parallelized over rows with rayon. A wgpu implementation can replace the
//! per-pixel kernels behind the same interface; this module remains the
//! reference for parity tests.

use rayon::prelude::*;
use studio_timeline::FitMode;

use crate::state::ClipState;

/// A decoded source frame in RGBA8.
pub struct SourceFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Factor by which the frame was decoded larger than the base fit/cover size.
    pub headroom: f32,
    pub fit: FitMode,
}

/// RGBA8 canvas.
#[derive(Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, bg: [u8; 4]) -> Self {
        let mut data = vec![0u8; (width * height * 4) as usize];
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&bg);
        }
        Self {
            width,
            height,
            data,
        }
    }
    pub fn fill(&mut self, bg: [u8; 4]) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&bg);
        }
    }
}

#[inline]
fn sample_bilinear(src: &SourceFrame, x: f32, y: f32) -> [f32; 4] {
    let w = src.width as i32;
    let h = src.height as i32;
    // Pixel centres at +0.5.
    let fx = x - 0.5;
    let fy = y - 0.5;
    if fx < -0.5 || fy < -0.5 || fx > w as f32 - 0.5 || fy > h as f32 - 0.5 {
        return [0.0; 4];
    }
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let get = |xx: i32, yy: i32| -> [f32; 4] {
        let xx = xx.clamp(0, w - 1) as usize;
        let yy = yy.clamp(0, h - 1) as usize;
        let i = (yy * w as usize + xx) * 4;
        let d = &src.data[i..i + 4];
        [d[0] as f32, d[1] as f32, d[2] as f32, d[3] as f32]
    };
    let a = get(x0, y0);
    let b = get(x0 + 1, y0);
    let c = get(x0, y0 + 1);
    let d = get(x0 + 1, y0 + 1);
    let mut o = [0f32; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bot = c[k] + (d[k] - c[k]) * tx;
        o[k] = top + (bot - top) * ty;
    }
    o
}

/// Geometry mapping canvas pixel → source pixel for a state.
struct Mapping {
    cx_src: f32,
    cy_src: f32,
    inv_scale: f32,
    cos: f32,
    sin: f32,
    shift_x: f32,
    shift_y: f32,
    half_w: f32,
    half_h: f32,
}

fn mapping(src: &SourceFrame, st: &ClipState, cw: u32, ch: u32) -> Mapping {
    let (cwf, chf) = (cw as f32, ch as f32);
    // Decoded frames are already at (fit|cover size) × headroom.
    let scale = st.zoom.max(0.01) / src.headroom.max(0.01);
    let half_w = cwf * 0.5;
    let half_h = chf * 0.5;
    let mut cx = st.center_x * src.width as f32;
    let mut cy = st.center_y * src.height as f32;
    if src.fit == FitMode::Cover {
        // Keep the visible window inside the source (no empty borders) when possible.
        let vis_hw = half_w / scale;
        let vis_hh = half_h / scale;
        let (sw, sh) = (src.width as f32, src.height as f32);
        cx = if vis_hw * 2.0 >= sw {
            sw * 0.5
        } else {
            cx.clamp(vis_hw, sw - vis_hw)
        };
        cy = if vis_hh * 2.0 >= sh {
            sh * 0.5
        } else {
            cy.clamp(vis_hh, sh - vis_hh)
        };
    }
    let rad = st.rotation_deg.to_radians();
    Mapping {
        cx_src: cx,
        cy_src: cy,
        inv_scale: 1.0 / scale,
        cos: rad.cos(),
        sin: rad.sin(),
        shift_x: (st.shake_x * chf) + st.offset_x * cwf,
        shift_y: (st.shake_y * chf) + st.offset_y * chf,
        half_w,
        half_h,
    }
}

#[inline]
fn map_px(m: &Mapping, x: f32, y: f32) -> (f32, f32) {
    let dx = x - m.half_w - m.shift_x;
    let dy = y - m.half_h - m.shift_y;
    let rx = dx * m.cos + dy * m.sin;
    let ry = -dx * m.sin + dy * m.cos;
    (m.cx_src + rx * m.inv_scale, m.cy_src + ry * m.inv_scale)
}

#[inline]
fn color_ops(mut rgb: [f32; 3], st: &ClipState, vig: f32) -> [f32; 3] {
    // contrast around mid-grey, brightness offset, saturation around luma
    for c in rgb.iter_mut() {
        *c = ((*c / 255.0 - 0.5) * st.contrast + 0.5 + st.brightness) * 255.0;
    }
    if (st.saturation - 1.0).abs() > 1e-4 {
        let l = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        for c in rgb.iter_mut() {
            *c = l + (*c - l) * st.saturation;
        }
    }
    if vig > 0.0 {
        for c in rgb.iter_mut() {
            *c *= 1.0 - vig;
        }
    }
    if st.flash > 0.0 {
        for c in rgb.iter_mut() {
            *c += (255.0 - *c) * st.flash;
        }
    }
    if st.black > 0.0 {
        for c in rgb.iter_mut() {
            *c *= 1.0 - st.black;
        }
    }
    rgb
}

/// Render one clip layer into an RGBA float-free buffer (straight alpha) of canvas size.
/// Pixels outside the source get alpha 0.
pub fn render_layer(src: &SourceFrame, st: &ClipState, cw: u32, ch: u32, out: &mut [u8]) {
    if st.has_spatial_fx() {
        return render_layer_fx(src, st, cw, ch, out);
    }
    let m = mapping(src, st, cw, ch);
    let split_px = st.rgb_split * cw as f32 * m.inv_scale;
    let color = st.has_color_ops();
    let diag = (m.half_w * m.half_w + m.half_h * m.half_h).sqrt();
    out.par_chunks_exact_mut((cw * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let yf = y as f32 + 0.5;
            for x in 0..cw as usize {
                let xf = x as f32 + 0.5;
                let (sx, sy) = map_px(&m, xf, yf);
                let mut px = sample_bilinear(src, sx, sy);
                if split_px.abs() > 0.01 {
                    let r = sample_bilinear(src, sx + split_px, sy);
                    let b = sample_bilinear(src, sx - split_px, sy);
                    px[0] = r[0];
                    px[2] = b[2];
                    px[3] = px[3].max(r[3]).max(b[3]);
                }
                write_px(&mut row[x * 4..x * 4 + 4], px, st, color, xf, yf, &m, diag);
            }
        });
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn write_px(
    o: &mut [u8],
    px: [f32; 4],
    st: &ClipState,
    color: bool,
    xf: f32,
    yf: f32,
    m: &Mapping,
    diag: f32,
) {
    let mut rgb = [px[0], px[1], px[2]];
    if color {
        let vig = if st.vignette > 0.0 {
            let dx = xf - m.half_w;
            let dy = yf - m.half_h;
            let r = (dx * dx + dy * dy).sqrt() / diag;
            st.vignette * ((r - 0.45) / 0.55).clamp(0.0, 1.0).powf(1.6)
        } else {
            0.0
        };
        rgb = color_ops(rgb, st, vig);
    }
    o[0] = rgb[0].clamp(0.0, 255.0).round() as u8;
    o[1] = rgb[1].clamp(0.0, 255.0).round() as u8;
    o[2] = rgb[2].clamp(0.0, 255.0).round() as u8;
    o[3] = px[3].clamp(0.0, 255.0).round() as u8;
}

fn hash01(a: u32, b: u32, c: u32) -> f32 {
    let mut x =
        a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

const BLUR_TAPS: usize = 9;

/// Slow path for spatial effects: glitch, chromatic aberration, directional and zoom blur.
/// All offsets are defined in canvas space so results are resolution-independent.
fn render_layer_fx(src: &SourceFrame, st: &ClipState, cw: u32, ch: u32, out: &mut [u8]) {
    let m = mapping(src, st, cw, ch);
    let color = st.has_color_ops();
    let diag = (m.half_w * m.half_w + m.half_h * m.half_h).sqrt();
    let (cwf, chf) = (cw as f32, ch as f32);
    let blur = (st.blur_x * cwf, st.blur_y * cwf);
    let has_blur = blur.0.abs() + blur.1.abs() > 0.5;
    let has_zoom_blur = st.zoom_blur * diag > 0.5;
    let split_c = st.rgb_split * cwf;
    let band_h = (chf / 28.0).max(2.0);
    // Average of mapped samples around canvas point (cx, cy) along the active blur paths.
    let sample = |cx: f32, cy: f32| -> [f32; 4] {
        if !has_blur && !has_zoom_blur {
            let (sx, sy) = map_px(&m, cx, cy);
            return sample_bilinear(src, sx, sy);
        }
        let mut acc = [0f32; 4];
        for k in 0..BLUR_TAPS {
            let t = k as f32 / (BLUR_TAPS - 1) as f32;
            let (mut px, mut py) = (cx, cy);
            if has_blur {
                px += blur.0 * (t - 0.5);
                py += blur.1 * (t - 0.5);
            }
            if has_zoom_blur {
                let f = 1.0 - st.zoom_blur * t;
                px = m.half_w + (px - m.half_w) * f;
                py = m.half_h + (py - m.half_h) * f;
            }
            let (sx, sy) = map_px(&m, px, py);
            let s = sample_bilinear(src, sx, sy);
            for c in 0..4 {
                acc[c] += s[c];
            }
        }
        acc.map(|v| v / BLUR_TAPS as f32)
    };
    out.par_chunks_exact_mut((cw * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let yf = y as f32 + 0.5;
            // Glitch: whole horizontal bands jump sideways and split channels.
            let (mut gdx, mut gsplit) = (0.0f32, 0.0f32);
            if st.glitch > 1e-3 {
                let band = (yf / band_h) as u32;
                if hash01(st.glitch_seed, st.glitch_phase, band) < st.glitch * 0.55 {
                    gdx = (hash01(st.glitch_seed ^ 0xA5A5, st.glitch_phase, band) - 0.5)
                        * 2.0
                        * st.glitch
                        * 0.12
                        * cwf;
                    gsplit = st.glitch * 0.012 * cwf;
                }
            }
            for x in 0..cw as usize {
                let xf = x as f32 + 0.5 + gdx;
                let split = split_c + gsplit;
                let (dx, dy) = (xf - m.half_w, yf - m.half_h);
                let px = if st.chroma > 1e-5 || split.abs() > 0.01 {
                    let r = sample(
                        m.half_w + dx * (1.0 + st.chroma) + split,
                        m.half_h + dy * (1.0 + st.chroma),
                    );
                    let g = sample(xf, yf);
                    let b = sample(
                        m.half_w + dx * (1.0 - st.chroma) - split,
                        m.half_h + dy * (1.0 - st.chroma),
                    );
                    [r[0], g[1], b[2], g[3].max(r[3]).max(b[3])]
                } else {
                    sample(xf, yf)
                };
                write_px(
                    &mut row[x * 4..x * 4 + 4],
                    px,
                    st,
                    color,
                    x as f32 + 0.5,
                    yf,
                    &m,
                    diag,
                );
            }
        });
}

/// `dst = src over dst` with an extra global opacity.
pub fn blend_over(dst: &mut [u8], src: &[u8], opacity: f32) {
    let op = opacity.clamp(0.0, 1.0);
    dst.par_chunks_exact_mut(4)
        .zip(src.par_chunks_exact(4))
        .for_each(|(d, s)| {
            let a = s[3] as f32 / 255.0 * op;
            if a <= 0.0 {
                return;
            }
            for k in 0..3 {
                d[k] = (s[k] as f32 * a + d[k] as f32 * (1.0 - a)).round() as u8;
            }
            d[3] = 255;
        });
}

/// Mix two full layers: `dst = a*(1-t) + b*t` (used for dissolves).
pub fn mix(a: &[u8], b: &[u8], t: f32, dst: &mut [u8]) {
    let t = t.clamp(0.0, 1.0);
    dst.par_chunks_exact_mut(4)
        .zip(a.par_chunks_exact(4).zip(b.par_chunks_exact(4)))
        .for_each(|(d, (x, y))| {
            for k in 0..4 {
                d[k] = (x[k] as f32 * (1.0 - t) + y[k] as f32 * t).round() as u8;
            }
        });
}

/// Uniform overlay towards a colour (flash cuts, dips).
pub fn overlay(dst: &mut [u8], rgb: [u8; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    dst.par_chunks_exact_mut(4).for_each(|d| {
        for k in 0..3 {
            d[k] = (d[k] as f32 * (1.0 - a) + rgb[k] as f32 * a).round() as u8;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let c = if (x / 8 + y / 8) % 2 == 0 { 255 } else { 0 };
                v[i..i + 4].copy_from_slice(&[c, x as u8, y as u8, 255]);
            }
        }
        v
    }

    #[test]
    fn identity_mapping_reproduces_source() {
        let (w, h) = (64u32, 48u32);
        let data = checker(w, h);
        let src = SourceFrame {
            data: &data,
            width: w,
            height: h,
            headroom: 1.0,
            fit: FitMode::Cover,
        };
        let mut out = vec![0u8; data.len()];
        render_layer(&src, &ClipState::identity(), w, h, &mut out);
        assert_eq!(out, data);
    }

    #[test]
    fn cover_crop_respects_focus_and_bounds() {
        // Wide source (128x48) into a 48x48 canvas: visible window is 48 px wide.
        let (w, h) = (128u32, 48u32);
        let data = checker(w, h);
        let src = SourceFrame {
            data: &data,
            width: w,
            height: h,
            headroom: 1.0,
            fit: FitMode::Cover,
        };
        let mut out = vec![0u8; 48 * 48 * 4];
        let mut st = ClipState::identity();
        st.center_x = 1.0; // far right: must clamp so the window stays inside
        render_layer(&src, &st, 48, 48, &mut out);
        // Rightmost canvas pixel shows source x = 127 (green channel stores x).
        let i = (10 * 48 + 47) * 4;
        assert_eq!(out[i + 1], 127);
        assert_eq!(out[i + 3], 255);
    }

    #[test]
    fn motion_blur_averages_along_direction() {
        // Vertical stripes 1 px wide: horizontal blur averages them to grey, vertical blur does not.
        let (w, h) = (32u32, 32u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let c = if x % 2 == 0 { 255 } else { 0 };
                data[i..i + 4].copy_from_slice(&[c, c, c, 255]);
            }
        }
        let src = SourceFrame {
            data: &data,
            width: w,
            height: h,
            headroom: 1.0,
            fit: FitMode::Cover,
        };
        let mut out = vec![0u8; data.len()];
        let mut st = ClipState::identity();
        st.blur_x = 8.0 / w as f32;
        render_layer(&src, &st, w, h, &mut out);
        let i = ((16 * w + 16) * 4) as usize;
        assert!(
            (out[i] as i32 - 128).abs() < 40,
            "horizontal blur -> grey, got {}",
            out[i]
        );
        st.blur_x = 0.0;
        st.blur_y = 8.0 / w as f32;
        render_layer(&src, &st, w, h, &mut out);
        assert!(
            out[i] == 255 || out[i] == 0,
            "vertical blur keeps stripes, got {}",
            out[i]
        );
    }

    #[test]
    fn glitch_and_chroma_change_pixels_deterministically() {
        let (w, h) = (64u32, 64u32);
        let data = checker(w, h);
        let src = SourceFrame {
            data: &data,
            width: w,
            height: h,
            headroom: 1.0,
            fit: FitMode::Cover,
        };
        let mut a = vec![0u8; data.len()];
        let mut b = vec![0u8; data.len()];
        let mut st = ClipState::identity();
        st.glitch = 1.0;
        st.glitch_seed = 9;
        render_layer(&src, &st, w, h, &mut a);
        render_layer(&src, &st, w, h, &mut b);
        assert_eq!(a, b);
        assert_ne!(a, data);
        st.glitch = 0.0;
        st.chroma = 0.05;
        render_layer(&src, &st, w, h, &mut a);
        assert_ne!(a, data);
    }

    #[test]
    fn flash_and_black_overlays() {
        let data = vec![100u8; 4 * 4 * 4];
        let src = SourceFrame {
            data: &data,
            width: 4,
            height: 4,
            headroom: 1.0,
            fit: FitMode::Cover,
        };
        let mut out = vec![0u8; data.len()];
        let mut st = ClipState::identity();
        st.flash = 1.0;
        render_layer(&src, &st, 4, 4, &mut out);
        assert_eq!(out[0], 255);
        st.flash = 0.0;
        st.black = 1.0;
        render_layer(&src, &st, 4, 4, &mut out);
        assert_eq!(out[0], 0);
    }
}
