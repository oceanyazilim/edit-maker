use studio_core::{Rational, Tick};
use studio_vision::{CutKind, SceneAnalyzer, VisionConfig};

const W: usize = 160;
const H: usize = 90;

fn solid(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(W * H * 3);
    for y in 0..H {
        for x in 0..W {
            // gentle gradient so the frame is not perfectly flat
            let k = ((x + y) % 32) as u8 / 4;
            v.extend_from_slice(&[
                r.saturating_add(k),
                g.saturating_add(k),
                b.saturating_add(k),
            ]);
        }
    }
    v
}

fn texture(shift_x: i32, seed: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(W * H * 3);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let xs = x - shift_x;
            let c = (((xs.div_euclid(10)) + (y / 10)) % 2 == 0) as u8;
            let n = ((xs.wrapping_mul(73) ^ y.wrapping_mul(151) ^ seed as i32) & 31) as u8;
            let base = if c == 1 { 190 } else { 50 };
            v.extend_from_slice(&[base + n / 2, base / 2 + n, 120]);
        }
    }
    v
}

fn moving_square(frame: usize) -> Vec<u8> {
    let mut v = solid(20, 30, 90);
    let cx = (120.0 + (frame as f32 * 0.9).sin() * 18.0) as usize;
    let cy = (45.0 + (frame as f32 * 1.3).cos() * 12.0) as usize;
    for y in cy.saturating_sub(10)..(cy + 10).min(H) {
        for x in cx.saturating_sub(10)..(cx + 10).min(W) {
            let i = (y * W + x) * 3;
            v[i] = 240;
            v[i + 1] = 220;
            v[i + 2] = 40;
        }
    }
    v
}

fn blend(a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f32 * (1.0 - t) + *y as f32 * t).round() as u8)
        .collect()
}

#[test]
fn detects_cuts_black_gradual_motion_and_focus() {
    let fps = Rational::new(15, 1).unwrap();
    let mut an = SceneAnalyzer::new(VisionConfig::default(), fps, W as u32, H as u32);
    let mut f = 0usize;
    // Scene A: static red, with a single white flash frame at 20.
    for i in 0..40 {
        if i == 20 {
            an.push(&solid(250, 250, 250));
        } else {
            an.push(&solid(160, 40, 40));
        }
        f += 1;
    }
    let cut_b = f; // 40
    for i in 0..40 {
        an.push(&texture(i as i32 * 2, 7));
        f += 1;
    }
    let black_start = f; // 80
    for _ in 0..6 {
        an.push(&solid(0, 0, 0).iter().map(|_| 2u8).collect::<Vec<_>>());
        f += 1;
    }
    let cut_c = f; // 86
    for i in 0..45 {
        an.push(&moving_square(i));
        f += 1;
    }
    // Dissolve C -> D over 15 frames.
    let c_last = moving_square(45);
    let d = solid(40, 170, 60);
    let dissolve_mid = f + 7;
    for i in 0..15 {
        an.push(&blend(&c_last, &d, (i + 1) as f32 / 16.0));
        f += 1;
    }
    for _ in 0..30 {
        an.push(&d);
        f += 1;
    }
    let res = an.finish();
    let fr = |t: Tick| t.to_frame_round(fps) as usize;
    let cuts: Vec<(usize, CutKind)> = res.cuts.iter().map(|c| (fr(c.t), c.kind)).collect();
    println!("cuts {cuts:?}");
    println!(
        "scenes {:?}",
        res.scenes
            .iter()
            .map(|s| (fr(s.start), fr(s.end), s.motion, s.focus_x, s.tags.clone()))
            .collect::<Vec<_>>()
    );

    assert!(
        !cuts.iter().any(|c| c.0 >= 19 && c.0 <= 22),
        "flash frame must not be a cut"
    );
    assert!(
        cuts.iter().any(|c| c.0 == cut_b && c.1 == CutKind::Hard),
        "hard cut at {cut_b}"
    );
    assert!(cuts.iter().any(|c| c.0 == black_start), "black start");
    assert!(
        cuts.iter().any(|c| c.0 == cut_c),
        "black end / scene C start"
    );
    assert!(
        cuts.iter()
            .any(|c| c.1 == CutKind::Gradual && (c.0 as i32 - dissolve_mid as i32).abs() <= 6),
        "gradual near {dissolve_mid}"
    );

    let scene_b = res.scenes.iter().find(|s| fr(s.start) == cut_b).unwrap();
    assert!(
        scene_b.tags.iter().any(|t| t == "pan_right"),
        "pan detected: {:?} dx={}",
        scene_b.tags,
        scene_b.camera_dx
    );
    let scene_black = res
        .scenes
        .iter()
        .find(|s| fr(s.start) == black_start)
        .unwrap();
    assert!(!scene_black.usable);
    let scene_c = res.scenes.iter().find(|s| fr(s.start) == cut_c).unwrap();
    assert!(scene_c.focus_x > 0.6, "focus_x {}", scene_c.focus_x);
    let scene_a = &res.scenes[0];
    assert!(
        scene_a.motion < scene_b.motion,
        "static scene must have less motion"
    );
    assert_eq!(res.frame_count as usize, f);
}

#[test]
fn single_static_scene() {
    let fps = Rational::new(10, 1).unwrap();
    let mut an = SceneAnalyzer::new(VisionConfig::default(), fps, W as u32, H as u32);
    for _ in 0..30 {
        an.push(&solid(90, 90, 90));
    }
    let r = an.finish();
    assert_eq!(r.scenes.len(), 1);
    assert!(r.cuts.is_empty());
    assert!(r.scenes[0].tags.iter().any(|t| t == "low_motion"));
}
