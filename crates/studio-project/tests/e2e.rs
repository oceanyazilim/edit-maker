//! End-to-end: real media processing, no mocks.
//!
//! music (synthetic WAV with known structure) + 3 generated test videos
//! → audio analysis → scene analysis → plan → render MP4 → validate → reopen project.
//!
//! Requires FFmpeg/FFprobe (STUDIO_FFMPEG_DIR, third_party/ffmpeg or PATH).
//! When they are missing the test prints SKIPPED and returns: this is reported
//! as "not executed", never as passed, in IMPLEMENTATION_STATUS.md.

use std::path::PathBuf;

use studio_audio::synth;
use studio_core::cancel::noop_progress;
use studio_core::{CancelToken, Tick};
use studio_media::FfmpegTools;
use studio_project::engine::RegenMode;
use studio_project::{store, AppDirs, AssetRole, Engine, ProjectSettings};
use studio_render::RenderSettings;

fn tools() -> Option<FfmpegTools> {
    match FfmpegTools::locate(&[]) {
        Ok(t) => Some(t),
        Err(_) => {
            eprintln!("SKIPPED: FFmpeg not found — end-to-end test was NOT executed.");
            None
        }
    }
}

#[test]
fn music_plus_footage_to_validated_mp4() {
    let Some(tools) = tools() else { return };
    let root = std::env::temp_dir().join("studio-e2e");
    let _ = std::fs::remove_dir_all(&root);
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    let cancel = CancelToken::new();

    // Music: 128 BPM arrangement with a drop at 30.5 s.
    let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 11);
    let music = fixtures.join("music.wav");
    studio_audio::decode::write_wav_mono(&music, &tr.samples, 44100).unwrap();

    // Footage: three 1920x1080 videos with internal hard cuts and different motion.
    let vids: Vec<(PathBuf, Vec<(f64, String)>)> = vec![
        (
            fixtures.join("a.mp4"),
            vec![
                (6.0, "testsrc2".into()),
                (6.0, "smptehdbars".into()),
                (6.0, "rgbtestsrc".into()),
            ],
        ),
        (
            fixtures.join("b.mp4"),
            vec![(8.0, "mandelbrot".into()), (8.0, "testsrc".into())],
        ),
        (
            fixtures.join("c.mp4"),
            vec![
                (5.0, "cellauto=rule=110".into()),
                (5.0, "life=ratio=0.2:mold=10".into()),
                (5.0, "testsrc2".into()),
            ],
        ),
    ];
    for (p, segs) in &vids {
        studio_media::transcode::synth_test_video(&tools, p, 1280, 720, 30, segs, &cancel)
            .expect("generate test video");
    }

    let engine = Engine::new(AppDirs::under(&root.join("app"))).unwrap();
    let settings = ProjectSettings {
        duration_secs: 12.0,
        preset_id: "high-energy-edm".into(),
        seed: 7,
        width: 540,
        height: 960,
        ..Default::default()
    };
    let mut project = engine.create_project("E2E Test", settings).unwrap();
    engine
        .import_media(&mut project, &music, AssetRole::Music, &cancel)
        .unwrap();
    for (p, _) in &vids {
        let a = engine
            .import_media(&mut project, p, AssetRole::Footage, &cancel)
            .unwrap();
        assert!(
            a.thumbnail.as_ref().map(|t| t.exists()).unwrap_or(false),
            "thumbnail generated"
        );
    }

    // 1. Audio analysis.
    let m = project.music().unwrap().clone();
    let audio = engine
        .audio_analysis(&m, &Default::default(), &cancel, &noop_progress())
        .unwrap();
    assert!(
        (audio.tempo.bpm - 128.0).abs() < 1.0,
        "bpm {}",
        audio.tempo.bpm
    );
    assert!(audio.main_drop().is_some());

    // 2. Scenes: internal cuts must be detected.
    let f0 = project.footage().next().unwrap().clone();
    let va = engine
        .video_analysis(&f0, &cancel, &noop_progress())
        .unwrap();
    println!(
        "video a: {} scenes, cuts at {:?}",
        va.scenes.len(),
        va.cuts
            .iter()
            .map(|c| c.t.as_seconds_f64())
            .collect::<Vec<_>>()
    );
    assert!(
        va.cuts
            .iter()
            .any(|c| (c.t.as_seconds_f64() - 6.0).abs() < 0.15),
        "cut at 6 s"
    );
    assert!(
        va.cuts
            .iter()
            .any(|c| (c.t.as_seconds_f64() - 12.0).abs() < 0.15),
        "cut at 12 s"
    );

    // 3. Plan → timeline.
    let plan = engine
        .generate(&mut project, &cancel, &noop_progress())
        .unwrap();
    let clips = &project.timeline.tracks[0].clips;
    println!("plan: {} clips, warnings {:?}", clips.len(), plan.warnings);
    assert!(clips.len() >= 4);
    assert_eq!(project.timeline.duration(), Tick::from_seconds_f64(12.0));

    // 4. Preview frame (same compositor as export).
    let jpg = engine
        .preview_jpeg(&project, Tick::from_seconds_f64(3.0), 0.5)
        .unwrap();
    assert!(jpg.len() > 1000 && jpg[0] == 0xFF && jpg[1] == 0xD8);

    // 5. Render a real MP4 and 6. validate it.
    let out = root.join("out").join("e2e.mp4");
    let rs = RenderSettings {
        output: out.clone(),
        encoder: "auto".into(),
        quality: 23,
        speed_preset: "fast".into(),
        ..Default::default()
    };
    let rep = engine
        .render(&mut project, &rs, &cancel, &noop_progress())
        .unwrap();
    println!(
        "render: {} frames in {:.1}s ({:.1} fps), encoder {}, warnings {:?}",
        rep.frames, rep.elapsed_secs, rep.render_fps, rep.encoder, rep.warnings
    );
    assert!(out.is_file());
    assert!(rep.check.ok, "{:?}", rep.check.problems);
    assert_eq!((rep.check.width, rep.check.height), (540, 960));
    assert!(rep.check.has_audio);
    assert!((rep.check.duration.as_seconds_f64() - 12.0).abs() < 0.1);
    assert!(!store::partial_exists(&out));

    // 7. Reopen the project.
    engine.save_project(&mut project).unwrap();
    let reopened = engine.open_project(project.file.as_ref().unwrap()).unwrap();
    assert_eq!(reopened.project.timeline, project.timeline);
    assert_eq!(reopened.project.renders.len(), 1);
    assert!(reopened.missing_assets.is_empty());

    // Cached analyses are reused (no re-analysis of unchanged files).
    let t0 = std::time::Instant::now();
    engine
        .video_analysis(&f0, &cancel, &noop_progress())
        .unwrap();
    assert!(t0.elapsed().as_millis() < 500);
}

#[test]
fn render_cancellation_leaves_no_partial_file() {
    let Some(tools) = tools() else { return };
    let root = std::env::temp_dir().join("studio-e2e-cancel");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cancel = CancelToken::new();
    let v = root.join("v.mp4");
    studio_media::transcode::synth_test_video(
        &tools,
        &v,
        640,
        360,
        30,
        &[(10.0, "testsrc2".into())],
        &cancel,
    )
    .unwrap();
    let tr = synth::render(&synth::edm_arrangement()[2..3], 128.0, 44100, 0.2, 3);
    let music = root.join("m.wav");
    studio_audio::decode::write_wav_mono(&music, &tr.samples, 44100).unwrap();
    let engine = Engine::new(AppDirs::under(&root.join("app"))).unwrap();
    let mut p = engine
        .create_project(
            "Cancel",
            ProjectSettings {
                duration_secs: 8.0,
                width: 360,
                height: 640,
                ..Default::default()
            },
        )
        .unwrap();
    engine
        .import_media(&mut p, &music, AssetRole::Music, &cancel)
        .unwrap();
    engine
        .import_media(&mut p, &v, AssetRole::Footage, &cancel)
        .unwrap();
    engine.generate(&mut p, &cancel, &noop_progress()).unwrap();
    let out = root.join("cancel.mp4");
    let c2 = CancelToken::new();
    let c3 = c2.clone();
    let progress: studio_core::ProgressSink =
        std::sync::Arc::new(move |pr: studio_core::Progress| {
            if pr.fraction.unwrap_or(0.0) > 0.2 {
                c3.cancel();
            }
        });
    let r = engine.render(
        &mut p,
        &RenderSettings {
            output: out.clone(),
            encoder: "auto".into(),
            ..Default::default()
        },
        &c2,
        &progress,
    );
    assert!(r.unwrap_err().is_cancelled());
    assert!(!out.exists());
    assert!(!store::partial_exists(&out));
}

/// V4 acceptance core: lock the first 5 s, regenerate the rest, locked part unchanged,
/// versions recoverable, manual audio corrections persist across save/reopen, real render.
#[test]
fn lock_regenerate_versions_and_audio_edits() {
    let Some(tools) = tools() else { return };
    let root = std::env::temp_dir().join("studio-e2e-locks");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cancel = CancelToken::new();
    let tr = synth::render(&synth::edm_arrangement(), 128.0, 44100, 0.5, 21);
    let music = root.join("music.wav");
    studio_audio::decode::write_wav_mono(&music, &tr.samples, 44100).unwrap();
    let mut vids = vec![];
    for (i, src) in ["testsrc2", "mandelbrot", "smptehdbars"].iter().enumerate() {
        let v = root.join(format!("v{i}.mp4"));
        studio_media::transcode::synth_test_video(
            &tools,
            &v,
            640,
            360,
            30,
            &[(8.0, src.to_string()), (8.0, "rgbtestsrc".into())],
            &cancel,
        )
        .unwrap();
        vids.push(v);
    }
    let engine = Engine::new(AppDirs::under(&root.join("app"))).unwrap();
    let mut p = engine
        .create_project(
            "Locks",
            ProjectSettings {
                duration_secs: 15.0,
                width: 360,
                height: 640,
                preset_id: "aggressive-phonk".into(),
                ..Default::default()
            },
        )
        .unwrap();
    engine
        .import_media(&mut p, &music, AssetRole::Music, &cancel)
        .unwrap();
    for v in &vids {
        engine
            .import_media(&mut p, v, AssetRole::Footage, &cancel)
            .unwrap();
    }

    // Segment suggestions come from the detected structure.
    let segs = engine.suggest_segments(&p, 15.0).unwrap();
    assert!(!segs.is_empty());
    println!(
        "segments: {:?}",
        segs.iter()
            .map(|s| (s.label.clone(), s.start_secs, s.score))
            .collect::<Vec<_>>()
    );

    engine.generate(&mut p, &cancel, &noop_progress()).unwrap();
    let five = Tick::from_seconds_f64(5.0);
    let first = p.timeline.tracks[0]
        .clips
        .iter()
        .filter(|c| c.end() <= five)
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    assert!(!first.is_empty());
    p.timeline.lock_range("V1", Tick::ZERO, five, true).unwrap();
    let locked_before: Vec<_> = p.timeline.tracks[0]
        .clips
        .iter()
        .filter(|c| c.locked)
        .cloned()
        .collect();
    let rest_before: Vec<_> = p.timeline.tracks[0]
        .clips
        .iter()
        .filter(|c| !c.locked)
        .map(|c| (c.start, c.asset_id.clone(), c.source_in))
        .collect();

    engine
        .regenerate(&mut p, &RegenMode::KeepLocked, &cancel, &noop_progress())
        .unwrap();
    for lc in &locked_before {
        let now = p.timeline.tracks[0]
            .clips
            .iter()
            .find(|c| c.id == lc.id)
            .expect("locked clip kept");
        assert_eq!(now, lc, "locked clip changed during regeneration");
    }
    let rest_after: Vec<_> = p.timeline.tracks[0]
        .clips
        .iter()
        .filter(|c| !c.locked)
        .map(|c| (c.start, c.asset_id.clone(), c.source_in))
        .collect();
    assert_ne!(rest_before, rest_after, "unlocked part did not change");
    assert_eq!(p.versions.len(), 1, "previous timeline saved as a version");

    // Range regeneration keeps everything outside 6–10 s; temporary locks do not stick.
    let (r0, r1) = (Tick::from_seconds_f64(6.0), Tick::from_seconds_f64(10.0));
    let outside: Vec<_> = p.timeline.tracks[0]
        .clips
        .iter()
        .filter(|c| !(c.start >= r0 && c.end() <= r1))
        .cloned()
        .collect();
    engine
        .regenerate(
            &mut p,
            &RegenMode::Range {
                start_secs: 6.0,
                end_secs: 10.0,
            },
            &cancel,
            &noop_progress(),
        )
        .unwrap();
    for c in &outside {
        let now = p.timeline.tracks[0]
            .clips
            .iter()
            .find(|x| x.id == c.id)
            .expect("outside clip kept");
        assert_eq!(
            (
                now.start,
                now.duration,
                now.source_in,
                &now.asset_id,
                &now.effects
            ),
            (c.start, c.duration, c.source_in, &c.asset_id, &c.effects)
        );
        assert_eq!(now.locked, c.locked, "temporary lock leaked");
    }

    // Restore the first version.
    let v0 = p.versions[0].clone();
    engine.restore_version(&mut p, &v0.id).unwrap();
    assert_eq!(p.timeline, v0.timeline);

    // Manual audio correction: remove the detected drops and add one at 50 s.
    let a0 = engine
        .corrected_audio_analysis(
            p.music().unwrap(),
            &p.settings.audio_edits,
            &cancel,
            &noop_progress(),
        )
        .unwrap();
    p.settings.audio_edits.removed_drops_secs =
        a0.drops.iter().map(|d| a0.seconds(d.sample)).collect();
    p.settings.audio_edits.added_drops_secs = vec![50.0];
    p.settings.drop_position = Some(0.5);
    engine.save_project(&mut p).unwrap();
    let mut p2 = engine
        .open_project(p.file.as_ref().unwrap())
        .unwrap()
        .project;
    assert_eq!(
        p2.settings.audio_edits.added_drops_secs,
        vec![50.0],
        "correction survives reopen"
    );
    assert_eq!(p2.versions.len(), p.versions.len());
    engine.generate(&mut p2, &cancel, &noop_progress()).unwrap();
    let drop = p2
        .timeline
        .markers
        .iter()
        .find(|m| m.label == "Main drop")
        .expect("manual drop used")
        .t;
    // Window start + drop position 0.5 -> the 50 s drop sits mid-edit (within a bar, after downbeat snapping).
    println!(
        "manual drop placed at {:.2}s of a {:.2}s edit",
        drop.as_seconds_f64(),
        p2.timeline.duration().as_seconds_f64()
    );
    assert!((drop.as_seconds_f64() - 7.5).abs() < 2.0);

    // Render the regenerated edit for real.
    let out = root.join("locks.mp4");
    let rep = engine
        .render(
            &mut p2,
            &RenderSettings {
                output: out.clone(),
                encoder: "auto".into(),
                speed_preset: "fast".into(),
                ..Default::default()
            },
            &cancel,
            &noop_progress(),
        )
        .unwrap();
    assert!(rep.check.ok && out.is_file());
}

/// Export dialog backends: range, other aspect ratio (reframed), fps override, audio-only, still.
#[test]
fn export_overrides_audio_and_still() {
    let Some(tools) = tools() else { return };
    let root = std::env::temp_dir().join("studio-e2e-export");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cancel = CancelToken::new();
    let tr = synth::render(&synth::edm_arrangement()[1..3], 128.0, 44100, 0.2, 5);
    let music = root.join("m.wav");
    studio_audio::decode::write_wav_mono(&music, &tr.samples, 44100).unwrap();
    let v = root.join("v.mp4");
    studio_media::transcode::synth_test_video(
        &tools,
        &v,
        640,
        360,
        30,
        &[(10.0, "testsrc2".into()), (10.0, "smptehdbars".into())],
        &cancel,
    )
    .unwrap();
    let engine = Engine::new(AppDirs::under(&root.join("app"))).unwrap();
    let mut p = engine
        .create_project(
            "Export",
            ProjectSettings {
                duration_secs: 8.0,
                width: 360,
                height: 640,
                ..Default::default()
            },
        )
        .unwrap();
    engine
        .import_media(&mut p, &music, AssetRole::Music, &cancel)
        .unwrap();
    engine
        .import_media(&mut p, &v, AssetRole::Footage, &cancel)
        .unwrap();
    engine.generate(&mut p, &cancel, &noop_progress()).unwrap();

    use studio_project::engine::ExportOverrides;
    let o = ExportOverrides {
        range: Some((Tick::from_seconds_f64(2.0), Tick::from_seconds_f64(5.0))),
        width: Some(480),
        height: Some(480),
        fps: Some(studio_core::Rational::FPS_60),
        prefer_hardware: false,
    };
    let out = root.join("range_square_60.mp4");
    let rep = engine
        .render_with(
            &mut p,
            &RenderSettings {
                output: out.clone(),
                encoder: "auto".into(),
                speed_preset: "fast".into(),
                ..Default::default()
            },
            &o,
            &cancel,
            &noop_progress(),
        )
        .unwrap();
    assert!(rep.check.ok, "{:?}", rep.check.problems);
    assert_eq!((rep.check.width, rep.check.height), (480, 480));
    assert_eq!(rep.check.fps.unwrap().as_f64(), 60.0);
    assert!(
        (rep.check.duration.as_seconds_f64() - 3.0).abs() < 0.06,
        "{}",
        rep.check.duration.as_seconds_f64()
    );
    assert_eq!(rep.frames, 180);

    for (fmt, ext) in [("m4a", "m4a"), ("wav", "wav")] {
        let a = root.join(format!("audio.{ext}"));
        engine
            .export_audio(
                &p,
                &ExportOverrides::default(),
                &a,
                fmt,
                192,
                false,
                &cancel,
                &noop_progress(),
            )
            .unwrap();
        let m = studio_media::probe(&tools, &a).unwrap();
        assert!(m.has_audio() && !m.has_video());
        assert!(
            (m.duration.as_seconds_f64() - 8.0).abs() < 0.1,
            "{fmt}: {}",
            m.duration.as_seconds_f64()
        );
    }
    let png = root.join("frame.png");
    engine
        .export_still(
            &p,
            &ExportOverrides::default(),
            Tick::from_seconds_f64(3.0),
            &png,
            false,
        )
        .unwrap();
    let img = image::open(&png).unwrap();
    assert_eq!((img.width(), img.height()), (360, 640));
}
