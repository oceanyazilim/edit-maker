//! `aes` – AI Auto Edit Studio command line.
//!
//!   aes doctor
//!   aes presets
//!   aes analyze-audio <file> [--json out.json]
//!   aes analyze-video <file>
//!   aes make-fixtures <dir>
//!   aes auto-edit --music <file> (--video <file> | --video-dir <dir>)... --out <file.mp4>
//!                 [--preset id] [--duration secs] [--aspect 9:16|16:9|1:1|4:5]
//!                 [--seed n] [--variant balanced|maximum_impact|...] [--niche id]
//!                 [--cue SECONDS[@FILE[:SOURCE_SECONDS]][!]]... [--start SONG_SECONDS] [--drop-pos 0..1]
//!   aes segments <music> [seconds...]

use std::path::PathBuf;
use std::sync::Arc;

use studio_core::{CancelToken, Progress, ProgressSink, StudioResult};
use studio_planner::Variant;
use studio_project::{resolution_for, AppDirs, AssetRole, Engine, ProjectSettings};
use studio_render::RenderSettings;

fn progress_printer() -> ProgressSink {
    let last = Arc::new(std::sync::Mutex::new((String::new(), -1i32)));
    Arc::new(move |p: Progress| {
        let pct = p.fraction.map(|f| (f * 100.0) as i32).unwrap_or(-1);
        let mut l = last.lock().unwrap();
        if l.0 != p.stage || pct / 10 != l.1 / 10 {
            eprintln!("  [{:>3}%] {}", pct.max(0), p.stage);
            *l = (p.stage.clone(), pct);
        }
    })
}

fn arg_values(args: &[String], name: &str) -> Vec<String> {
    let mut v = vec![];
    let mut i = 0;
    while i < args.len() {
        if args[i] == name && i + 1 < args.len() {
            v.push(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    v
}

fn arg(args: &[String], name: &str) -> Option<String> {
    arg_values(args, name).into_iter().next()
}

fn engine() -> StudioResult<Engine> {
    Engine::new(AppDirs::default_for_user())
}

fn run(args: Vec<String>) -> StudioResult<()> {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    let cancel = CancelToken::new();
    match cmd {
        "doctor" => {
            println!("AI Auto Edit Studio {}", env!("CARGO_PKG_VERSION"));
            println!(
                "CPU threads: {}",
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            );
            let e = engine()?;
            println!("Data dir:     {}", e.dirs.data.display());
            println!("Projects dir: {}", e.dirs.projects.display());
            match e.tools() {
                Ok(t) => {
                    println!(
                        "FFmpeg:  {} ({})\n  {}\n  {}",
                        t.version,
                        t.source,
                        t.ffmpeg.display(),
                        t.ffprobe.display()
                    );
                    let gpl = t
                        .build_configuration()
                        .map(|c| c.contains("--enable-gpl"))
                        .unwrap_or(false);
                    println!(
                        "  build: {}",
                        if gpl {
                            "GPL (see DEPENDENCIES.md before redistribution)"
                        } else {
                            "LGPL/unknown"
                        }
                    );
                    for enc in e.encoders()? {
                        println!(
                            "  encoder {:<12} {:<5} listed={} available={}",
                            enc.name,
                            if enc.hardware { "hw" } else { "sw" },
                            enc.listed,
                            enc.available
                        );
                    }
                }
                Err(err) => println!("FFmpeg:  NOT FOUND – {}", err.message),
            }
        }
        "presets" => {
            for p in engine()?.presets() {
                println!("{:<28} {}", p.id, p.description);
            }
        }
        "analyze-audio" => {
            let path = PathBuf::from(
                args.get(1)
                    .ok_or_else(|| studio_core::StudioError::invalid_input("missing file"))?,
            );
            let a = studio_audio::analyze_file(
                &path,
                &Default::default(),
                &cancel,
                &progress_printer(),
            )?;
            println!(
                "BPM {:.2} (confidence {:.2}), {} beats, {} kicks, {} snares",
                a.tempo.bpm,
                a.tempo.confidence,
                a.beats.len(),
                a.kicks.len(),
                a.snares.len()
            );
            for s in &a.sections {
                println!(
                    "  section {:>7.2}s – {:>7.2}s  {:?}  energy {:.2}",
                    a.seconds(s.start_sample),
                    a.seconds(s.end_sample),
                    s.kind,
                    s.energy
                );
            }
            for d in &a.drops {
                println!(
                    "  drop    {:>7.2}s  {:?}  score {:.2}",
                    a.seconds(d.sample),
                    d.kind,
                    d.score
                );
            }
            for w in &a.warnings {
                println!("  warning: {w}");
            }
            if let Some(out) = arg(&args, "--json") {
                std::fs::write(out, serde_json::to_vec_pretty(&a)?)?;
            }
        }
        "analyze-video" => {
            let e = engine()?;
            let t = e.tools()?;
            let path = studio_media::validate_input_path(&PathBuf::from(
                args.get(1)
                    .ok_or_else(|| studio_core::StudioError::invalid_input("missing file"))?,
            ))?;
            let m = studio_media::probe(&t, &path)?;
            println!("{:?}", m.video);
            let a = studio_media::analyze::analyze_video(
                &t,
                &path,
                &m,
                &Default::default(),
                &cancel,
                &progress_printer(),
            )?;
            for s in &a.scenes {
                println!(
                    "  scene {:>3} {:>7.2}s–{:>7.2}s motion {:.2} energy {:.2} quality {:.2} focus ({:.2},{:.2}) {:?}",
                    s.index,
                    s.start.as_seconds_f64(),
                    s.end.as_seconds_f64(),
                    s.motion,
                    s.energy,
                    s.quality,
                    s.focus_x,
                    s.focus_y,
                    s.tags
                );
            }
        }
        "segments" => {
            // Smart segment suggestions for a music file: aes segments <file> [seconds...]
            let path = PathBuf::from(
                args.get(1)
                    .ok_or_else(|| studio_core::StudioError::invalid_input("missing file"))?,
            );
            let a = studio_audio::analyze_file(
                &path,
                &Default::default(),
                &cancel,
                &studio_core::cancel::noop_progress(),
            )?;
            println!(
                "{:.2} BPM, main drop {:?}",
                a.tempo.bpm,
                a.main_drop()
                    .map(|d| (a.seconds(d.sample) * 100.0).round() / 100.0)
            );
            let durs: Vec<f64> = args.iter().skip(2).filter_map(|x| x.parse().ok()).collect();
            for d in if durs.is_empty() {
                vec![10.0, 15.0, 20.0, 30.0]
            } else {
                durs
            } {
                println!("--- {d}s");
                for s in studio_planner::recommend_segments(&a, d) {
                    println!(
                        "  {:<22} {:>7.2}s–{:>7.2}s  score {:.2}  drop {}  [{}]",
                        s.label,
                        s.start_secs,
                        s.end_secs,
                        s.score,
                        s.drop_offset_secs
                            .map(|x| format!("+{x:.2}s"))
                            .unwrap_or_else(|| "-".into()),
                        s.reasons.join("; ")
                    );
                }
            }
        }
        "verify-sync" => {
            // Independent check on a rendered file: visual cuts (scene analyzer on the decoded
            // video) vs. musical events (audio analyzer on the file's own audio track).
            let e = engine()?;
            let t = e.tools()?;
            let path = studio_media::validate_input_path(&PathBuf::from(
                args.get(1)
                    .ok_or_else(|| studio_core::StudioError::invalid_input("missing file"))?,
            ))?;
            let m = studio_media::probe(&t, &path)?;
            let v = studio_media::analyze::analyze_video(
                &t,
                &path,
                &m,
                &Default::default(),
                &cancel,
                &studio_core::cancel::noop_progress(),
            )?;
            let a = studio_audio::analyze_file(
                &path,
                &Default::default(),
                &cancel,
                &studio_core::cancel::noop_progress(),
            )?;
            let beats: Vec<f64> = a.beats.iter().map(|b| a.seconds(b.sample)).collect();
            let kicks: Vec<f64> = a.kicks.iter().map(|k| a.seconds(k.sample)).collect();
            let nearest =
                |x: f64, v: &[f64]| v.iter().map(|y| (y - x).abs()).fold(f64::MAX, f64::min);
            let frame = 1.0 / m.video.as_ref().map(|v| v.fps.as_f64()).unwrap_or(30.0);
            println!(
                "audio: {:.2} BPM, {} beats, {} kicks; video: {} cuts (analysis fps {})",
                a.tempo.bpm,
                beats.len(),
                kicks.len(),
                v.cuts.len(),
                v.analysis_fps
            );
            let mut aligned = 0;
            for c in &v.cuts {
                let ct = c.t.as_seconds_f64();
                let (db, dk) = (nearest(ct, &beats), nearest(ct, &kicks));
                // Analysis runs at <=15 fps, so a cut is resolved to ~1 analysis frame (67 ms).
                let tol = 1.0 / v.analysis_fps.as_f64() + frame;
                let ok = db.min(dk) <= tol;
                aligned += ok as usize;
                println!(
                    "  cut {:>6.3}s {:?}  nearest beat {:>5.0} ms  nearest kick {:>5.0} ms  {}",
                    ct,
                    c.kind,
                    db * 1000.0,
                    dk * 1000.0,
                    if ok { "ALIGNED" } else { "off-beat" }
                );
            }
            println!(
                "aligned cuts: {}/{} (tolerance = 1 analysis frame + 1 video frame)",
                aligned,
                v.cuts.len()
            );
        }
        "make-fixtures" => {
            let dir = PathBuf::from(
                args.get(1)
                    .cloned()
                    .unwrap_or_else(|| "fixtures/generated".into()),
            );
            std::fs::create_dir_all(&dir)?;
            let tr = studio_audio::synth::render(
                &studio_audio::synth::edm_arrangement(),
                128.0,
                44100,
                0.5,
                11,
            );
            studio_audio::decode::write_wav_mono(
                &dir.join("music_128bpm.wav"),
                &tr.samples,
                44100,
            )?;
            std::fs::write(
                dir.join("music_128bpm.annotations.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"bpm": 128, "beats": tr.beats, "downbeats": tr.downbeats, "kicks": tr.kicks, "drops": tr.drops}),
                )?,
            )?;
            println!("wrote {}", dir.join("music_128bpm.wav").display());
            if let Ok(t) = engine()?.tools() {
                for (name, segs) in [
                    (
                        "clip_a.mp4",
                        vec![
                            (6.0, "testsrc2".to_string()),
                            (6.0, "smptehdbars".into()),
                            (6.0, "rgbtestsrc".into()),
                        ],
                    ),
                    (
                        "clip_b.mp4",
                        vec![(8.0, "mandelbrot".into()), (8.0, "testsrc".into())],
                    ),
                    (
                        "clip_c.mp4",
                        vec![
                            (5.0, "cellauto=rule=110".into()),
                            (5.0, "life=ratio=0.2:mold=10".into()),
                        ],
                    ),
                ] {
                    studio_media::transcode::synth_test_video(
                        &t,
                        &dir.join(name),
                        1920,
                        1080,
                        30,
                        &segs,
                        &cancel,
                    )?;
                    println!("wrote {}", dir.join(name).display());
                }
            } else {
                println!("FFmpeg not found: video fixtures skipped");
            }
        }
        "auto-edit" => {
            let e = engine()?;
            let music = arg(&args, "--music")
                .ok_or_else(|| studio_core::StudioError::invalid_input("--music is required"))?;
            let videos = arg_values(&args, "--video");
            let out = PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "auto-edit.mp4".into()));
            let aspect = arg(&args, "--aspect").unwrap_or_else(|| "9:16".into());
            let (w, h) = resolution_for(&aspect);
            let variant: Variant = arg(&args, "--variant")
                .map(|v| serde_json::from_value(serde_json::Value::String(v)))
                .transpose()?
                .unwrap_or(Variant::Balanced);
            let settings = ProjectSettings {
                aspect,
                width: w,
                height: h,
                duration_secs: arg(&args, "--duration")
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(20.0),
                preset_id: arg(&args, "--preset").unwrap_or_else(|| "high-energy-edm".into()),
                seed: arg(&args, "--seed")
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(1),
                niche_id: arg(&args, "--niche"),
                audio_start_secs: arg(&args, "--start").and_then(|d| d.parse().ok()),
                drop_position: arg(&args, "--drop-pos").and_then(|d| d.parse().ok()),
                variant,
                ..Default::default()
            };
            let mut p = e.create_project(
                out.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .as_deref()
                    .unwrap_or("auto-edit"),
                settings,
            )?;
            eprintln!("Importing media…");
            e.import_media(&mut p, &PathBuf::from(music), AssetRole::Music, &cancel)?;
            let mut imported: Vec<(String, String)> = vec![]; // (file name, asset id)
            for v in videos {
                let asset =
                    e.import_media(&mut p, &PathBuf::from(v), AssetRole::Footage, &cancel)?;
                imported.push((asset.name.clone(), asset.id.clone()));
            }
            // --video-dir DIR: every video in the folder (e.g. the day's clips).
            for dir in arg_values(&args, "--video-dir") {
                let (imp, skipped, errs) =
                    e.import_folder(&mut p, &PathBuf::from(&dir), &cancel)?;
                eprintln!(
                    "  {dir}: {} imported, {skipped} already present, {} failed",
                    imp.len(),
                    errs.len()
                );
                for er in errs {
                    eprintln!("    {er}");
                }
                imported.extend(imp.into_iter().map(|a| (a.name, a.id)));
            }
            // --cue SECONDS[@FILE[:SOURCE_SECONDS]][!]   ("!" = exact time, do not snap to the beat)
            for spec in arg_values(&args, "--cue") {
                let (spec, snap) = match spec.strip_suffix('!') {
                    Some(s) => (s.to_string(), false),
                    None => (spec.clone(), true),
                };
                let (at, rest) = spec
                    .split_once('@')
                    .map(|(a, r)| (a.to_string(), Some(r.to_string())))
                    .unwrap_or((spec.clone(), None));
                let at_secs: f64 = at.trim().parse().map_err(|_| {
                    studio_core::StudioError::invalid_input(format!("bad --cue time: {at}"))
                })?;
                let (asset_id, source_secs) = match rest {
                    None => (None, None),
                    Some(r) => {
                        let (file, src) = match r.rsplit_once(':') {
                            Some((f, s)) if s.trim().parse::<f64>().is_ok() => {
                                (f.to_string(), s.trim().parse::<f64>().ok())
                            }
                            _ => (r.clone(), None),
                        };
                        let id = imported
                            .iter()
                            .find(|(n, _)| {
                                n.eq_ignore_ascii_case(&file)
                                    || PathBuf::from(&file)
                                        .file_name()
                                        .map(|f| f.to_string_lossy().eq_ignore_ascii_case(n))
                                        .unwrap_or(false)
                            })
                            .map(|(_, id)| id.clone())
                            .ok_or_else(|| {
                                studio_core::StudioError::invalid_input(format!(
                                    "--cue refers to {file}, which is not one of the --video files"
                                ))
                            })?;
                        (Some(id), src)
                    }
                };
                p.settings.cues.push(studio_planner::Cue {
                    at_secs,
                    asset_id,
                    source_secs,
                    snap_to_beat: snap,
                    label: String::new(),
                });
            }
            eprintln!("Analyzing and planning…");
            let plan = e.generate(&mut p, &cancel, &progress_printer())?;
            for s in &plan.segments {
                eprintln!(
                    "  {:>6.2}s–{:>6.2}s  {:<10} {}",
                    s.start.as_seconds_f64(),
                    s.end.as_seconds_f64(),
                    format!("{:?}", s.section),
                    s.reason
                );
            }
            for w in &plan.warnings {
                eprintln!("  warning: {w}");
            }
            for c in &plan.cues {
                eprintln!(
                    "  cue requested {:.3}s -> placed {:.3}s (clip #{:?}), source moment honored: {}",
                    c.requested.as_seconds_f64(),
                    c.resolved.as_seconds_f64(),
                    c.clip_index,
                    c.honored_source
                );
            }
            eprintln!("Rendering…");
            let rep = e.render(
                &mut p,
                &RenderSettings {
                    output: out.clone(),
                    encoder: "auto".into(),
                    overwrite: true,
                    ..Default::default()
                },
                &cancel,
                &progress_printer(),
            )?;
            e.save_project(&mut p)?;
            println!(
                "OK {} — {}x{}, {:.2}s, {} frames, encoder {}, {:.1} render fps, validated: {}",
                rep.output.display(),
                rep.width,
                rep.height,
                rep.duration.as_seconds_f64(),
                rep.frames,
                rep.encoder,
                rep.render_fps,
                rep.check.ok
            );
            for w in rep.warnings {
                println!("  warning: {w}");
            }
            println!("Project saved: {}", p.file.unwrap().display());
        }
        _ => {
            println!(
                "{}",
                include_str!("main.rs")
                    .lines()
                    .skip(2)
                    .take(9)
                    .map(|l| l.trim_start_matches("//!"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(args) {
        eprintln!("Error: {}", e.message);
        if let Some(d) = e.details {
            eprintln!("Details: {d}");
        }
        std::process::exit(1);
    }
}
