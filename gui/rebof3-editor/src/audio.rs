//! Plays decoded PCM on the default output device.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The sample rate of a VAG at pitch `0x1000`.
pub const RATE: u32 = 44_100;

/// The scale from a 16-bit sample to the -1.0..1.0 float range.
const SAMPLE_SCALE: f32 = 32_768.0;

#[derive(Default)]
pub struct Player {
    stream: Option<cpal::Stream>,
    clip: Arc<Mutex<Arc<Vec<i16>>>>,
    position: Arc<AtomicUsize>,
}

impl Player {
    /// Starts playing mono 16-bit PCM at [`RATE`], replacing the clip that plays now.
    pub fn play(&mut self, pcm: Vec<i16>) -> Result<(), String> {
        *self.clip.lock().map_err(|_| "audio state poisoned")? = Arc::new(pcm);
        self.position.store(0, Ordering::Relaxed);
        if self.stream.is_none() {
            self.stream = Some(self.open()?);
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }

    pub fn is_playing(&self) -> bool {
        let length = self.clip.lock().map(|clip| clip.len()).unwrap_or(0);
        self.stream.is_some() && self.position.load(Ordering::Relaxed) < length
    }

    fn open(&self) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        let supported = device.default_output_config().map_err(|error| error.to_string())?;
        let channels = usize::from(supported.channels());
        let rate = supported.sample_rate();
        let config: cpal::StreamConfig = supported.into();
        let clip = self.clip.clone();
        let position = self.position.clone();
        // A simple nearest-sample resampler: the editor previews sounds, it does not master them.
        let step = f64::from(RATE) / f64::from(rate);
        let mut phase = 0.0f64;
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _| {
                    let Ok(clip) = clip.lock() else { return };
                    let clip = clip.clone();
                    for frame in out.chunks_mut(channels) {
                        let at = position.load(Ordering::Relaxed);
                        let sample = clip.get(at).map_or(0.0, |&value| f32::from(value) / SAMPLE_SCALE);
                        frame.fill(sample);
                        phase += step;
                        let advance = phase as usize;
                        phase -= advance as f64;
                        position.store(at + advance, Ordering::Relaxed);
                    }
                },
                |error| eprintln!("bof3-edit: audio: {error}"),
                None,
            )
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(stream)
    }
}
