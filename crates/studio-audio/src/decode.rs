//! Audio decoding through Symphonia (pure Rust; WAV, MP3, FLAC, AAC/M4A, OGG Vorbis…).
//! Produces a mono f32 analysis signal at the *native* sample rate so event
//! positions map 1:1 to source samples.

use std::fs::File;
use std::path::Path;

use studio_core::{CancelToken, ErrorCode, StudioError, StudioResult};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Clone, Debug)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Mono downmix (mean of channels), not normalized.
    pub samples: Vec<f32>,
    pub codec: String,
    /// Number of corrupt packets that were skipped.
    pub skipped_packets: u32,
}

impl DecodedAudio {
    pub fn from_mono(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 1,
            samples,
            codec: "pcm_f32".into(),
            skipped_packets: 0,
        }
    }
    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

fn unsupported(e: impl ToString) -> StudioError {
    StudioError::new(
        ErrorCode::UnsupportedFormat,
        "Unsupported or unrecognized audio format.",
    )
    .with_details(e.to_string())
}

pub fn decode_file(path: &Path, cancel: &CancelToken) -> StudioResult<DecodedAudio> {
    let file = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StudioError::not_found("Source media could not be found.")
                .with_details(path.display().to_string())
        } else {
            e.into()
        }
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(unsupported)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| {
            StudioError::invalid_media("The file does not contain a decodable audio track.")
        })?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let codec_name = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map(|d| d.short_name.to_string())
        .unwrap_or_else(|| "unknown".into());
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(unsupported)?;
    let sample_rate = params.sample_rate.unwrap_or(0);
    let mut channels = params.channels.map(|c| c.count() as u16).unwrap_or(0);

    let mut samples: Vec<f32> =
        Vec::with_capacity(params.n_frames.unwrap_or(0).min(1 << 28) as usize);
    let mut buf: Option<SampleBuffer<f32>> = None;
    let mut buf_frames = 0usize;
    let mut skipped = 0u32;
    let mut packets = 0u64;

    loop {
        if packets % 64 == 0 {
            cancel.check()?;
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(SymError::DecodeError(_)) => {
                skipped += 1;
                continue;
            }
            Err(e) => {
                if samples.is_empty() {
                    return Err(
                        StudioError::invalid_media("The audio stream could not be read.")
                            .with_details(e.to_string()),
                    );
                }
                tracing::warn!("stopping decode early: {e}");
                break;
            }
        };
        packets += 1;
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let n_ch = spec.channels.count().max(1);
                channels = n_ch as u16;
                let cap = decoded.capacity();
                if buf.is_none() || cap > buf_frames {
                    buf = Some(SampleBuffer::new(cap as u64, spec));
                    buf_frames = cap;
                }
                let b = buf.as_mut().expect("buffer");
                b.copy_interleaved_ref(decoded);
                let inv = 1.0 / n_ch as f32;
                for frame in b.samples().chunks_exact(n_ch) {
                    samples.push(frame.iter().sum::<f32>() * inv);
                }
            }
            Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                skipped += 1;
                if skipped > 1000 && samples.is_empty() {
                    return Err(StudioError::invalid_media(
                        "The audio stream appears to be corrupted.",
                    ));
                }
            }
            Err(e) => {
                return Err(StudioError::invalid_media("Audio decoding failed.")
                    .with_details(e.to_string()));
            }
        }
    }
    if samples.is_empty() || sample_rate == 0 {
        return Err(StudioError::invalid_media(
            "The audio stream contains no samples.",
        ));
    }
    // Replace non-finite values defensively (corrupt streams).
    for s in samples.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    Ok(DecodedAudio {
        sample_rate,
        channels,
        samples,
        codec: codec_name,
        skipped_packets: skipped,
    })
}

/// Write a mono 16-bit WAV (used for fixtures and diagnostics).
pub fn write_wav_mono(path: &Path, samples: &[f32], sample_rate: u32) -> StudioResult<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| {
        StudioError::new(ErrorCode::Io, "Could not write WAV file.").with_details(e.to_string())
    })?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        w.write_sample(v).map_err(|e| {
            StudioError::new(ErrorCode::Io, "Could not write WAV file.").with_details(e.to_string())
        })?;
    }
    w.finalize().map_err(|e| {
        StudioError::new(ErrorCode::Io, "Could not write WAV file.").with_details(e.to_string())
    })?;
    Ok(())
}
