//! Hears what this computer is playing.
//!
//! Spotify's streams are protected, so the app cannot decode them itself. When the official
//! Spotify app plays on this computer, the spectrum instead follows a loopback recording of the
//! audio output. Nothing is kept: each block goes straight to the analyser.

use crate::spectrum;
use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub(crate) struct Listener {
    // Recording stops when the stream is dropped.
    _stream: cpal::Stream,
}

impl Listener {
    pub(crate) fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let (device, config) = source(&host)?;
        let channels = usize::from(config.channels().max(1));
        let rate = config.sample_rate();
        let mut mono = Vec::new();
        let stream = device
            .build_input_stream(
                config.config(),
                move |samples: &[f32], _: &cpal::InputCallbackInfo| {
                    mono.clear();
                    mono.extend(
                        samples
                            .chunks_exact(channels)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32),
                    );
                    spectrum::feed(&mono, rate);
                },
                |_| {},
                None,
            )
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { _stream: stream })
    }
}

/// macOS (14.6 and later) and Windows record an output device directly.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn source(host: &cpal::Host) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let device = host
        .default_output_device()
        .ok_or("There is no audio output to listen to.")?;
    let config = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    Ok((device, config))
}

/// PulseAudio and PipeWire expose each output as a recordable monitor source.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn source(host: &cpal::Host) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let device = host
        .input_devices()
        .map_err(|error| error.to_string())?
        .find(|device| {
            device
                .description()
                .is_ok_and(|description| description.to_string().to_lowercase().contains("monitor"))
        })
        .ok_or("There is no monitor source to listen to.")?;
    let config = device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    Ok((device, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plays a quiet tone and records the real audio output, so it only runs on request:
    // cargo test -- --ignored
    #[test]
    #[ignore = "needs an audio output device and permission to record it"]
    fn the_output_reaches_the_analyser() {
        use rodio::source::{SineWave, Source};
        let _listener = Listener::start().unwrap();
        // The tone goes straight to the mixer, so only the recording can report it.
        let output = rodio::DeviceSinkBuilder::from_default_device()
            .and_then(|builder| builder.open_sink_or_fallback())
            .unwrap();
        output.mixer().add(SineWave::new(440.0).amplify(0.1));
        std::thread::sleep(std::time::Duration::from_secs(2));
        let bands = spectrum::latest().expect("the recording delivers samples");
        assert!(bands.iter().any(|level| *level > 0.05), "{bands:?}");
    }
}
