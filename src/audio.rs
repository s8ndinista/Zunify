//! The desktop audio path. What the app plays itself is decoded in-process and mixed here before
//! it reaches the output device, which lets the spectrum analyser hear it.

use crate::spectrum;
use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
    source::SeekError,
};
use std::{io::Cursor, time::Duration};

/// Passes a source through unchanged while feeding a mono mix of it to the analyser.
///
/// The tap sits inside the sink, ahead of its volume control, so the spectrum follows the music
/// and not the volume slider. It only advances as the output mixer pulls samples, which keeps the
/// analysis within one device buffer of what is audible.
pub(crate) struct Tap<S> {
    inner: S,
    channels: u16,
    rate: u32,
    sum: f32,
    count: u16,
    block: Vec<f32>,
}

impl<S: Source> Tap<S> {
    const BLOCK: usize = 256;

    pub(crate) fn new(inner: S) -> Self {
        Self {
            channels: inner.channels().get(),
            rate: inner.sample_rate().get(),
            inner,
            sum: 0.0,
            count: 0,
            block: Vec::with_capacity(Self::BLOCK),
        }
    }

    fn flush(&mut self) {
        if !self.block.is_empty() {
            spectrum::feed(&self.block, self.rate);
            self.block.clear();
        }
    }
}

impl<S: Source> Iterator for Tap<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let Some(sample) = self.inner.next() else {
            self.flush();
            return None;
        };
        self.sum += sample;
        self.count += 1;
        if self.count == self.channels {
            self.block.push(self.sum / f32::from(self.channels));
            self.sum = 0.0;
            self.count = 0;
            if self.block.len() == Self::BLOCK {
                self.flush();
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Tap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.sum = 0.0;
        self.count = 0;
        self.block.clear();
        self.inner.try_seek(position)
    }
}

pub(crate) struct Audio {
    // The output must outlive the player connected to it.
    pub(crate) sink: Player,
    _output: MixerDeviceSink,
}

impl Audio {
    pub(crate) fn new() -> Result<Self, String> {
        // The default buffer is larger than some devices allow, so let the device settle it.
        let mut output = DeviceSinkBuilder::from_default_device()
            .and_then(|builder| builder.open_sink_or_fallback())
            .map_err(|error| format!("Could not open audio output: {error}"))?;
        output.log_on_drop(false);
        Ok(Self {
            sink: Player::connect_new(output.mixer()),
            _output: output,
        })
    }

    /// Replaces whatever the main sink is playing with a bundled recording.
    pub(crate) fn play(&self, bytes: &'static [u8], volume: f64) -> Result<(), String> {
        let source = Decoder::try_from(Cursor::new(bytes))
            .map_err(|error| format!("Could not read demo audio: {error}"))?;
        self.sink.clear();
        self.sink.set_volume((volume / 100.0) as f32);
        self.sink.append(Tap::new(source));
        self.sink.play();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plays through the real output device, so it only runs on request:
    // cargo test -- --ignored
    #[test]
    #[ignore = "needs an audio output device"]
    fn bundled_audio_reaches_the_analyser_at_any_volume() {
        assert!(spectrum::latest().is_none());
        let audio = Audio::new().unwrap();
        audio
            .play(include_bytes!("../wwwroot/audio/sample-1.wav"), 0.0)
            .unwrap();
        // The recording fades in over its first second.
        std::thread::sleep(Duration::from_millis(1500));
        let bands = spectrum::latest().expect("the mixer feeds the analyser while playing");
        assert!(bands.iter().any(|level| *level > 0.1), "{bands:?}");
        audio.sink.pause();
        std::thread::sleep(Duration::from_millis(400));
        assert!(spectrum::latest().is_none(), "a paused sink goes quiet");
    }
}
