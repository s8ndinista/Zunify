//! Live spectrum analysis of whatever the app is playing.
//!
//! Both builds route their audio through the app before it reaches the output device. The desktop
//! build taps the samples on their way into the mixer; the browser build pushes the samples of
//! its Web Audio analyser. Either way the transport fire reads the same 64 bands from here.

use std::cell::RefCell;

pub(crate) const BANDS: usize = 64;
const WINDOW: usize = 2048;

struct Analyser {
    rate: u32,
    window: Vec<f32>,
    twiddles: Vec<(f32, f32)>,
    // First bin, one past the last bin, and the gain of each band.
    bands: Vec<(usize, usize, f32)>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Analyser {
    fn new(rate: u32) -> Self {
        let size = WINDOW as f32;
        let window = (0..WINDOW)
            .map(|index| 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / (size - 1.0)).cos())
            .collect();
        let twiddles = (0..WINDOW / 2)
            .map(|index| {
                let angle = -std::f32::consts::TAU * index as f32 / size;
                (angle.cos(), angle.sin())
            })
            .collect();
        // Geometric spacing keeps bass and treble legible in the same strip.
        let hz = rate.max(1) as f32;
        let top = (hz * 0.475).min(12_000.0);
        let first = ((31.0 * size / hz).round() as usize).max(1);
        let last = ((top * size / hz) as usize).clamp(first + BANDS, WINDOW / 2);
        let mut edges = vec![first];
        for band in 1..=BANDS {
            let edge = first as f32 * (last as f32 / first as f32).powf(band as f32 / BANDS as f32);
            let previous = edges[band - 1];
            edges.push((edge.round() as usize).max(previous + 1));
        }
        let bands = edges
            .windows(2)
            .map(|edge| {
                let gain = 12.0 + 6.0 * (edge[0] as f32 / first as f32).log2();
                (edge[0], edge[1].min(WINDOW / 2), gain)
            })
            .collect();
        Self {
            rate,
            window,
            twiddles,
            bands,
            re: vec![0.0; WINDOW],
            im: vec![0.0; WINDOW],
        }
    }

    /// Analyses the most recent window of mono samples, oldest first.
    fn analyse(&mut self, samples: &[f32]) -> [f32; BANDS] {
        let samples = &samples[samples.len().saturating_sub(WINDOW)..];
        let lead = WINDOW - samples.len();
        let bits = WINDOW.trailing_zeros();
        for index in 0..WINDOW {
            let value = if index < lead {
                0.0
            } else {
                samples[index - lead] * self.window[index]
            };
            let target = index.reverse_bits() >> (usize::BITS - bits);
            self.re[target] = if value.is_finite() { value } else { 0.0 };
            self.im[target] = 0.0;
        }
        let mut step = 2;
        while step <= WINDOW {
            let half = step / 2;
            let stride = WINDOW / step;
            for base in (0..WINDOW).step_by(step) {
                for offset in 0..half {
                    let (cos, sin) = self.twiddles[offset * stride];
                    let (a, b) = (base + offset, base + offset + half);
                    let re = self.re[b] * cos - self.im[b] * sin;
                    let im = self.re[b] * sin + self.im[b] * cos;
                    self.re[b] = self.re[a] - re;
                    self.im[b] = self.im[a] - im;
                    self.re[a] += re;
                    self.im[a] += im;
                }
            }
            step *= 2;
        }
        let mut output = [0.0; BANDS];
        for (level, &(start, end, gain)) in output.iter_mut().zip(&self.bands) {
            let power: f32 = (start..end)
                .map(|bin| self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin])
                .sum();
            let magnitude = power.sqrt() * 4.0 / WINDOW as f32;
            *level = 1.0 - (-magnitude * gain).exp();
        }
        output
    }
}

thread_local! {
    static ANALYSER: RefCell<Option<Analyser>> = const { RefCell::new(None) };
}

fn analyse(samples: &[f32], rate: u32) -> [f32; BANDS] {
    ANALYSER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|analyser| analyser.rate != rate) {
            *slot = Some(Analyser::new(rate));
        }
        slot.as_mut().unwrap().analyse(samples)
    })
}

#[cfg(target_os = "emscripten")]
mod live {
    use std::cell::RefCell;

    thread_local! {
        static SAMPLES: RefCell<(Vec<f32>, u32)> = const { RefCell::new((Vec::new(), 0)) };
    }

    /// Replaces the current window; an empty slice means nothing is audible.
    pub(crate) fn push(samples: &[f32], rate: u32) {
        SAMPLES.with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.0.clear();
            slot.0.extend_from_slice(samples);
            slot.1 = rate;
        });
    }

    pub(super) fn window<R>(read: impl FnOnce(&[f32], u32) -> R) -> Option<R> {
        SAMPLES.with(|slot| {
            let slot = slot.borrow();
            (!slot.0.is_empty() && slot.1 > 0).then(|| read(&slot.0, slot.1))
        })
    }
}

#[cfg(not(target_os = "emscripten"))]
mod live {
    use super::WINDOW;
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    struct Ring {
        samples: [f32; WINDOW],
        next: usize,
        rate: u32,
        written: Option<Instant>,
    }

    static RING: Mutex<Ring> = Mutex::new(Ring {
        samples: [0.0; WINDOW],
        next: 0,
        rate: 0,
        written: None,
    });

    /// Records mono samples as the output mixer pulls them.
    pub(crate) fn feed(samples: &[f32], rate: u32) {
        let Ok(mut ring) = RING.lock() else { return };
        if ring.rate != rate {
            ring.samples.fill(0.0);
            ring.rate = rate;
        }
        for &sample in samples {
            let next = ring.next;
            ring.samples[next] = sample;
            ring.next = (next + 1) % WINDOW;
        }
        ring.written = Some(Instant::now());
    }

    pub(super) fn window<R>(read: impl FnOnce(&[f32], u32) -> R) -> Option<R> {
        let mut ordered = [0.0; WINDOW];
        let rate = {
            let ring = RING.lock().ok()?;
            // A paused or drained sink stops pulling samples, which leaves the ring stale.
            if ring.rate == 0 || ring.written?.elapsed() > Duration::from_millis(150) {
                return None;
            }
            let (newer, older) = ring.samples.split_at(ring.next);
            ordered[..older.len()].copy_from_slice(older);
            ordered[older.len()..].copy_from_slice(newer);
            ring.rate
        };
        Some(read(&ordered, rate))
    }
}

#[cfg(not(target_os = "emscripten"))]
pub(crate) use live::feed;
#[cfg(target_os = "emscripten")]
pub(crate) use live::push;

/// The spectrum of the audio that is audible right now, if the app can hear any.
pub(crate) fn latest() -> Option<[f32; BANDS]> {
    live::window(analyse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32) -> Vec<f32> {
        (0..WINDOW)
            .map(|index| 0.6 * (std::f32::consts::TAU * hz * index as f32 / rate as f32).sin())
            .collect()
    }

    fn loudest(bands: &[f32; BANDS]) -> usize {
        (0..BANDS)
            .max_by(|a, b| bands[*a].total_cmp(&bands[*b]))
            .unwrap()
    }

    #[test]
    fn higher_tones_light_higher_bands_and_silence_lights_none() {
        for rate in [16_000, 44_100, 48_000] {
            let mut analyser = Analyser::new(rate);
            let low = analyser.analyse(&tone(110.0, rate));
            let high = analyser.analyse(&tone(3_520.0, rate));
            assert!(low[loudest(&low)] > 0.5, "{rate}");
            assert!(loudest(&low) < loudest(&high), "{rate}");
            assert!(high[loudest(&low)] < 0.05, "{rate}");
            let silence = analyser.analyse(&[0.0; WINDOW]);
            assert!(silence.iter().all(|level| *level == 0.0), "{rate}");
        }
    }

    #[test]
    fn short_windows_are_padded() {
        let mut analyser = Analyser::new(44_100);
        let bands = analyser.analyse(&tone(440.0, 44_100)[..300]);
        assert!(bands.iter().all(|level| level.is_finite()));
        assert!(bands[loudest(&bands)] > 0.05);
    }
}
