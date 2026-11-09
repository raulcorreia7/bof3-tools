//! Plays rendered PCM on the default output device.
//!
//! The stream opens at the clip's own rate when the device offers it, so the audio library's 44.1 kHz
//! output reaches the device sample for sample. On a device without that rate, [`convert`] resamples the
//! clip with a band-limited sinc filter (rubato's FFT resampler), which keeps the treble that linear
//! interpolation dulls and the aliasing it adds out of the audible band.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rebof3_assets::sound::Pcm;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// The scale from a 16-bit sample to the -1.0..1.0 float range.
const SAMPLE_SCALE: f32 = 32_768.0;
/// The channel count a stream prefers: the library renders stereo.
const STEREO: u16 = 2;
/// Frames the resampler takes per FFT block; it rounds this up to suit the rate pair.
const RESAMPLE_CHUNK: usize = 1024;

/// Interleaved PCM as float frames of `channels` at `rate`, or why it cannot be resampled. Mono goes to
/// every channel; stereo goes to the first two and is averaged on a mono device. Another rate is reached
/// with rubato's FFT resampler, a band-limited sinc filter, over the whole clip.
pub fn convert(pcm: &Pcm, rate: u32, channels: usize) -> Result<Vec<f32>, String> {
    let source = pcm.channels.max(1) as usize;
    let mut stereo = Vec::with_capacity(pcm.samples.len() / source * 2);
    for frame in pcm.samples.chunks_exact(source) {
        let first = f32::from(frame[0]) / SAMPLE_SCALE;
        stereo.extend([first, frame.get(1).map_or(first, |&second| f32::from(second) / SAMPLE_SCALE)]);
    }
    let resampled = if pcm.sample_rate == rate || pcm.sample_rate == 0 || rate == 0 || stereo.is_empty() {
        stereo
    } else {
        resample(&stereo, pcm.sample_rate, rate)?
    };

    let mut out = Vec::with_capacity(resampled.len() / 2 * channels);
    for frame in resampled.as_chunks::<2>().0 {
        match channels {
            0 => {}
            1 => out.push((frame[0] + frame[1]) / 2.0),
            _ => {
                out.extend_from_slice(frame);
                out.extend(std::iter::repeat_n(0.0, channels - 2));
            }
        }
    }
    Ok(out)
}

/// Interleaved stereo at `from` Hz resampled to `to` Hz: the filter's delay trimmed, and as long as the clip.
fn resample(stereo: &[f32], from: u32, to: u32) -> Result<Vec<f32>, String> {
    let frames = stereo.len() / 2;
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, RESAMPLE_CHUNK, 2, FixedSync::Input)
        .map_err(|error| format!("resampling {from} Hz to {to} Hz: {error}"))?;
    let input = InterleavedSlice::new(stereo, 2, frames).map_err(|error| error.to_string())?;
    let output = resampler.process_all(&input, frames, None).map_err(|error| error.to_string())?;
    let mut out = output.take_data();
    out.truncate((frames as u64 * u64::from(to) / u64::from(from)) as usize * 2);
    Ok(out)
}

/// An open output stream and the shape it plays.
struct Output {
    _stream: cpal::Stream,
    rate: u32,
    channels: usize,
}

#[derive(Default)]
pub struct Player {
    output: Option<Output>,
    /// The clip as the stream plays it: float frames at the stream's rate and channel count.
    clip: Arc<Mutex<Arc<Vec<f32>>>>,
    position: Arc<AtomicUsize>,
}

impl Player {
    /// Starts playing `pcm`, replacing the clip that plays now.
    pub fn play(&mut self, pcm: &Pcm) -> Result<(), String> {
        if self.output.is_none() {
            self.output = Some(self.open(pcm.sample_rate)?);
        }
        let Some(output) = &self.output else { return Err("no audio output".into()) };
        let samples = Arc::new(convert(pcm, output.rate, output.channels)?);
        let mut clip = self.clip.lock().map_err(|_| "audio state poisoned")?;
        *clip = samples;
        self.position.store(0, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.output = None;
    }

    pub fn is_playing(&self) -> bool {
        let length = self.clip.lock().map(|clip| clip.len()).unwrap_or(0);
        self.output.is_some() && self.position.load(Ordering::Relaxed) < length
    }

    /// Opens the default output device at `rate` when it offers that rate, else at its default rate.
    fn open(&self, rate: u32) -> Result<Output, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        let config = stream_config(&device, rate)?;
        let (rate, channels) = (config.sample_rate, usize::from(config.channels));
        let clip = self.clip.clone();
        let position = self.position.clone();
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _| {
                    let Ok(clip) = clip.lock() else { return out.fill(0.0) };
                    let at = position.load(Ordering::Relaxed).min(clip.len());
                    let take = (clip.len() - at).min(out.len());
                    out[..take].copy_from_slice(&clip[at..at + take]);
                    out[take..].fill(0.0);
                    position.store(at + take, Ordering::Relaxed);
                },
                |error| eprintln!("bof3-edit: audio: {error}"),
                None,
            )
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Output { _stream: stream, rate, channels })
    }
}

/// The device's float output at `rate`, stereo when it can, or its default output when no float output
/// runs at `rate`.
fn stream_config(device: &cpal::Device, rate: u32) -> Result<cpal::StreamConfig, String> {
    let at_rate = device
        .supported_output_configs()
        .into_iter()
        .flatten()
        .filter(|range| range.sample_format() == cpal::SampleFormat::F32)
        .filter_map(|range| range.try_with_sample_rate(rate))
        .min_by_key(|config| config.channels().abs_diff(STEREO));
    match at_rate {
        Some(config) => Ok(config.config()),
        None => device.default_output_config().map(|config| config.config()).map_err(|error| error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;
    const DEVICE_RATE: u32 = 48_000;

    fn pcm(samples: Vec<i16>, sample_rate: u32, channels: u32) -> Pcm {
        Pcm { sample_rate, channels, samples }
    }

    /// One second of a sine at `hertz`, mono at `rate`.
    fn sine(hertz: f64, rate: u32) -> Vec<i16> {
        (0..rate)
            .map(|n| (8000.0 * (std::f64::consts::TAU * hertz * f64::from(n) / f64::from(rate)).sin()) as i16)
            .collect()
    }

    /// Rising zero crossings a second of `samples` holds.
    fn rising(samples: &[f32]) -> usize {
        samples.windows(2).filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0).count()
    }

    #[test]
    fn stereo_stays_stereo() {
        let out = convert(&pcm(vec![16_384, -16_384, 8_192, 0], RATE, 2), RATE, 2).unwrap();
        assert_eq!(out, [0.5, -0.5, 0.25, 0.0]);
    }

    #[test]
    fn mono_goes_to_both_sides_and_stereo_averages_on_a_mono_device() {
        assert_eq!(convert(&pcm(vec![16_384], RATE, 1), RATE, 2).unwrap(), [0.5, 0.5]);
        assert_eq!(convert(&pcm(vec![16_384, 0], RATE, 2), RATE, 1).unwrap(), [0.25]);
        assert_eq!(convert(&pcm(vec![16_384, 0], RATE, 2), RATE, 4).unwrap(), [0.5, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_resampler_keeps_a_tones_frequency() {
        let out = convert(&pcm(sine(1_000.0, RATE), RATE, 1), DEVICE_RATE, 1).unwrap();
        assert_eq!(out.len(), DEVICE_RATE as usize);
        let heard = rising(&out);
        assert!(heard.abs_diff(1_000) <= 1, "{heard} Hz after resampling, not 1000");
    }

    #[test]
    fn the_resampler_keeps_a_high_tones_level_and_the_sides_apart() {
        let left = sine(15_000.0, RATE);
        let samples = left.iter().flat_map(|&sample| [sample, 0]).collect();
        let out = convert(&pcm(samples, RATE, 2), DEVICE_RATE, 2).unwrap();
        let steady = |side: usize| out.iter().skip(side).step_by(2).skip(1_000).take(40_000).copied();
        let peak = steady(0).fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let input_peak = 8000.0 / SAMPLE_SCALE;
        assert!((peak - input_peak).abs() < input_peak * 0.05, "15 kHz peaks at {peak}, not {input_peak}");
        assert!(steady(1).all(|sample| sample.abs() < 1e-3), "the silent side stays silent");
    }

    #[test]
    fn the_resampler_keeps_the_clips_length() {
        let out = convert(&pcm(vec![0; 2 * RATE as usize], RATE, 2), DEVICE_RATE, 2).unwrap();
        assert_eq!(out.len(), 2 * DEVICE_RATE as usize);
    }
}
