use studio_audio::decode::{decode_file, write_wav_mono};
use studio_audio::synth;
use studio_core::{CancelToken, ErrorCode};

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join("studio-audio-tests");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

#[test]
fn wav_roundtrip() {
    let tr = synth::render(&synth::edm_arrangement()[2..3], 128.0, 44100, 0.1, 9);
    let p = tmp("roundtrip.wav");
    write_wav_mono(&p, &tr.samples, 44100).unwrap();
    let d = decode_file(&p, &CancelToken::new()).unwrap();
    assert_eq!(d.sample_rate, 44100);
    assert_eq!(d.channels, 1);
    assert_eq!(d.samples.len(), tr.samples.len());
    let err: f32 = d
        .samples
        .iter()
        .zip(tr.samples.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(err < 1e-3);
}

#[test]
fn corrupted_file_is_rejected_gracefully() {
    let p = tmp("corrupt.mp3");
    let junk: Vec<u8> = (0..20000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    std::fs::write(&p, junk).unwrap();
    let e = decode_file(&p, &CancelToken::new()).unwrap_err();
    assert!(
        matches!(
            e.code,
            ErrorCode::UnsupportedFormat | ErrorCode::InvalidMedia
        ),
        "{e:?}"
    );
}

#[test]
fn truncated_wav_is_handled() {
    let tr = synth::render(&synth::edm_arrangement()[2..3], 128.0, 44100, 0.1, 9);
    let p = tmp("full.wav");
    write_wav_mono(&p, &tr.samples, 44100).unwrap();
    let bytes = std::fs::read(&p).unwrap();
    let q = tmp("truncated.wav");
    std::fs::write(&q, &bytes[..bytes.len() / 2]).unwrap();
    // Either decodes the available half or reports invalid media – never panics.
    match decode_file(&q, &CancelToken::new()) {
        Ok(d) => assert!(d.samples.len() > 1000),
        Err(e) => assert!(matches!(
            e.code,
            ErrorCode::InvalidMedia | ErrorCode::UnsupportedFormat
        )),
    }
}

#[test]
fn missing_file_is_not_found() {
    let e = decode_file(
        std::path::Path::new("Z:/definitely/missing.wav"),
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
}
