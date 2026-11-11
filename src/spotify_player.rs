use crate::audio::{Audio, Tap};
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::{Session, SessionConfig, authentication::Credentials, config::DeviceType};
use librespot_playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    config::PlayerConfig,
    convert::Converter,
    decoder::AudioPacket,
    mixer::{Mixer, MixerConfig, NoOpVolume, softmixer::SoftMixer},
    player::{Player, PlayerEvent},
};
use rodio::{ChannelCount, SampleRate, buffer::SamplesBuffer};
use std::{
    sync::{
        Arc, Once,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, task::JoinHandle};

pub(crate) const LOCAL_DEVICE: &str = "zunify:local";

const TRACK_UNAVAILABLE: u8 = 1;
const KEY_DENIED: u8 = 2;
const OUTPUT_FAILED: u8 = 3;
const TRACK_NOTICE: &str = "Zunify could not load this Spotify track. Try another track or choose an output in Playback devices.";
const KEY_NOTICE: &str =
    "Spotify denied the audio key for Zunify. Choose another output in Playback devices to listen.";
const OUTPUT_NOTICE: &str = "Zunify could not play audio through this computer’s output. Check the system sound output and try again.";

// Librespot 0.8 exposes track failure as an event but only reports the server's key denial in its log.
// Listen for that single code; do not log account data, URLs or credentials.
static KEY_REJECTED: AtomicBool = AtomicBool::new(false);
struct AudioKeyLog;

impl log::Log for AudioKeyLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() == log::Level::Error && metadata.target() == "librespot_core::audio_key"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) && record.args().to_string() == "error audio key 0 1" {
            KEY_REJECTED.store(true, Ordering::Relaxed);
        }
    }

    fn flush(&self) {}
}

pub(crate) fn is_playback_notice(status: &str) -> bool {
    matches!(status, TRACK_NOTICE | KEY_NOTICE | OUTPUT_NOTICE)
}

pub(crate) struct SpotifyPlayer {
    session: Session,
    connect: Spirc,
    task: JoinHandle<()>,
    runtime: Option<Runtime>,
    fault: Arc<AtomicU8>,
}

impl SpotifyPlayer {
    pub(crate) fn connect(access_token: String, volume: f64) -> Result<Self, String> {
        static INIT_LOG: Once = Once::new();
        INIT_LOG.call_once(|| {
            if log::set_logger(&AudioKeyLog).is_ok() {
                log::set_max_level(log::LevelFilter::Error);
            }
        });
        KEY_REJECTED.store(false, Ordering::Relaxed);
        let fault = Arc::new(AtomicU8::new(0));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| format!("Could not start the local Spotify player: {error}"))?;
        let (session, connect, task) = runtime.block_on(async {
            let session = Session::new(SessionConfig::default(), None);
            let mixer = Arc::new(
                SoftMixer::open(MixerConfig::default()).map_err(|error| error.to_string())?,
            );
            let output_mixer = mixer.clone();
            let output_fault = fault.clone();
            let player = Player::new(
                PlayerConfig::default(),
                session.clone(),
                Box::new(NoOpVolume),
                move || {
                    Box::new(SpotifyOutput {
                        audio: None,
                        mixer: output_mixer,
                        fault: output_fault,
                    })
                },
            );
            let mut events = player.get_player_event_channel();
            let config = ConnectConfig {
                name: "Zunify".into(),
                device_type: DeviceType::Computer,
                initial_volume: (volume.clamp(0.0, 100.0) * f64::from(u16::MAX) / 100.0).round()
                    as u16,
                ..ConnectConfig::default()
            };
            let credentials = Credentials::with_access_token(access_token);
            let (connect, run) = tokio::time::timeout(
                Duration::from_secs(15),
                Spirc::new(config, session.clone(), credentials, player, mixer),
            )
            .await
            .map_err(|_| "Connecting to Spotify timed out.".to_owned())?
            .map_err(|error| format!("Spotify could not start the local player: {error}"))?;
            let event_fault = fault.clone();
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    if matches!(event, PlayerEvent::Unavailable { .. }) {
                        let code = if KEY_REJECTED.swap(false, Ordering::Relaxed) {
                            KEY_DENIED
                        } else {
                            TRACK_UNAVAILABLE
                        };
                        event_fault.fetch_max(code, Ordering::Relaxed);
                    }
                }
            });
            Ok::<_, String>((session, connect, tokio::spawn(run)))
        })?;
        Ok(Self {
            session,
            connect,
            task,
            runtime: Some(runtime),
            fault,
        })
    }

    pub(crate) fn device_id(&self) -> &str {
        self.session.device_id()
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.task.is_finished() && !self.session.is_invalid()
    }

    pub(crate) fn playback_notice(&self) -> Option<&'static str> {
        match self.fault.load(Ordering::Relaxed) {
            TRACK_UNAVAILABLE => Some(TRACK_NOTICE),
            KEY_DENIED => Some(KEY_NOTICE),
            OUTPUT_FAILED => Some(OUTPUT_NOTICE),
            _ => None,
        }
    }
}

impl Drop for SpotifyPlayer {
    fn drop(&mut self) {
        let _ = self.connect.shutdown();
        if let Some(runtime) = self.runtime.take() {
            let _ = runtime.block_on(async {
                tokio::time::timeout(Duration::from_millis(600), &mut self.task).await
            });
            self.session.shutdown();
            runtime.shutdown_background();
        }
    }
}

struct SpotifyOutput {
    audio: Option<Audio>,
    mixer: Arc<SoftMixer>,
    fault: Arc<AtomicU8>,
}

impl Sink for SpotifyOutput {
    fn start(&mut self) -> SinkResult<()> {
        if self.audio.is_none() {
            self.audio = Some(Audio::new().map_err(|error| {
                self.fault.store(OUTPUT_FAILED, Ordering::Relaxed);
                SinkError::ConnectionRefused(error)
            })?);
        }
        if let Some(audio) = &self.audio {
            audio.sink.play();
        }
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Some(audio) = &self.audio {
            audio.sink.clear();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let audio = self
            .audio
            .as_ref()
            .ok_or_else(|| SinkError::NotConnected("Audio output is closed.".into()))?;
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let source = SamplesBuffer::new(
            ChannelCount::new(NUM_CHANNELS as u16).unwrap(),
            SampleRate::new(SAMPLE_RATE).unwrap(),
            converter.f64_to_f32(samples),
        );
        audio
            .sink
            .set_volume(f32::from(self.mixer.volume()) / f32::from(u16::MAX));
        audio.sink.append(Tap::new(source));

        // Keep the decoder close to the device so pause and seek discard little buffered audio.
        let deadline = Instant::now() + Duration::from_secs(1);
        while audio.sink.len() > 8 {
            if Instant::now() >= deadline {
                self.fault.store(OUTPUT_FAILED, Ordering::Relaxed);
                return Err(SinkError::OnWrite(
                    "Audio output stopped accepting samples.".into(),
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.fault.store(0, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum;

    #[test]
    fn only_the_known_spotify_key_denial_sets_the_flag() {
        fn report(target: &str, code: u8) {
            log::Log::log(
                &AudioKeyLog,
                &log::Record::builder()
                    .level(log::Level::Error)
                    .target(target)
                    .args(format_args!("error audio key 0 {code:x}"))
                    .build(),
            );
        }
        report("unrelated", 1);
        report("librespot_core::audio_key", 2);
        assert!(!KEY_REJECTED.load(Ordering::Relaxed));
        report("librespot_core::audio_key", 1);
        assert!(KEY_REJECTED.swap(false, Ordering::Relaxed));
    }

    #[test]
    #[ignore = "needs an audio output device"]
    fn decoded_spotify_samples_reach_the_spectrum_at_zero_volume() {
        let mixer = Arc::new(SoftMixer::open(MixerConfig::default()).unwrap());
        mixer.set_volume(0);
        let mut output = SpotifyOutput {
            audio: None,
            mixer,
            fault: Arc::new(AtomicU8::new(0)),
        };
        output.start().unwrap();
        let mut converter = Converter::new(None);
        for chunk in 0..24 {
            let samples = (0..512)
                .flat_map(|frame| {
                    let t = f64::from(chunk * 512 + frame) / f64::from(SAMPLE_RATE);
                    let sample = (std::f64::consts::TAU * 440.0 * t).sin() * 0.5;
                    [sample, sample]
                })
                .collect();
            output
                .write(AudioPacket::Samples(samples), &mut converter)
                .unwrap();
        }
        thread::sleep(Duration::from_millis(60));
        let bands = spectrum::latest().expect("the native output feeds the spectrum");
        assert!(bands.iter().any(|level| *level > 0.1), "{bands:?}");
        output.stop().unwrap();
    }
}
