//! Plays rendered PCM on the default output device.
//!
//! The stream opens at the clip's own rate when the device offers it, so the audio library's 44.1 kHz
//! output reaches the device sample for sample. On a device without that rate, [`convert`] resamples by
//! linear interpolation, as bof3player's own output does.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rebof3_assets::sound::Pcm;

/// The scale from a 16-bit sample to the -1.0..1.0 float range.
const SAMPLE_SCALE: f32 = 32_768.0;
/// The channel count a stream prefers: the library renders stereo.
const STEREO: u16 = 2;

/// Interleaved PCM as float frames of `channels` at `rate`. Mono goes to every channel; stereo goes to the
/// first two and is averaged on a mono device. Another rate is reached by linear interpolation between
/// neighbouring frames.
pub fn convert(pcm: &Pcm, rate: u32, channels: usize) -> Vec<f32> {
    let source = pcm.channels.max(1) as usize;
    let frames = pcm.samples.len() / source;
    let frame = |at: usize| -> [f32; 2] {
        let first = f32::from(pcm.samples[at * source]) / SAMPLE_SCALE;
        let second = if source > 1 { f32::from(pcm.samples[at * source + 1]) / SAMPLE_SCALE } else { first };
        [first, second]
    };
    let same_rate = pcm.sample_rate == rate || pcm.sample_rate == 0 || rate == 0;
    let length =
        if same_rate { frames } else { (frames as u64 * u64::from(rate) / u64::from(pcm.sample_rate)) as usize };
    let step = if same_rate { 1.0 } else { f64::from(pcm.sample_rate) / f64::from(rate) };

    let mut out = Vec::with_capacity(length * channels);
    for at in 0..length {
        let position = at as f64 * step;
        let (before, weight) = (position as usize, position.fract() as f32);
        let (first, next) = (frame(before), frame((before + 1).min(frames - 1)));
        let [left, right] = [0, 1].map(|side| first[side] + (next[side] - first[side]) * weight);
        match channels {
            0 => {}
            1 => out.push((left + right) / 2.0),
            _ => {
                out.extend([left, right]);
                out.extend(std::iter::repeat_n(0.0, channels - 2));
            }
        }
    }
    out
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
        let samples = Arc::new(convert(pcm, output.rate, output.channels));
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
        let out = convert(&pcm(vec![16_384, -16_384, 8_192, 0], RATE, 2), RATE, 2);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.0]);
    }

    #[test]
    fn mono_goes_to_both_sides_and_stereo_averages_on_a_mono_device() {
        assert_eq!(convert(&pcm(vec![16_384], RATE, 1), RATE, 2), [0.5, 0.5]);
        assert_eq!(convert(&pcm(vec![16_384, 0], RATE, 2), RATE, 1), [0.25]);
        assert_eq!(convert(&pcm(vec![16_384, 0], RATE, 2), RATE, 4), [0.5, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_resampler_keeps_a_tones_frequency() {
        let out = convert(&pcm(sine(1_000.0, RATE), RATE, 1), DEVICE_RATE, 1);
        assert_eq!(out.len(), DEVICE_RATE as usize);
        let heard = rising(&out);
        assert!(heard.abs_diff(1_000) <= 1, "{heard} Hz after resampling, not 1000");
    }

    #[test]
    fn the_resampler_interpolates_between_frames() {
        let out = convert(&pcm(vec![0, 16_384], RATE, 1), RATE * 2, 1);
        assert_eq!(out, [0.0, 0.25, 0.5, 0.5]);
    }
}
