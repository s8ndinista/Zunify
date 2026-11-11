use ferroui_base::{
    Ref, WeakRef,
    media::{
        effects::{
            CustomEffect, EffectParameterValue, ImmutableCustomEffect, ShaderEffectDescription,
        },
        imaging::BitmapBlendingMode,
    },
    numerics::Vector3,
    rendering::composition::{ICompositionObjectAnimations, animations::AnimationDelayBehavior},
    threading::{Dispatcher, DispatcherPriority},
};
use ferroui_controls::{Border, TopLevel, hosting::ElementCompositionPreview};
use std::{cell::Cell, rc::Rc, time::Duration};

#[derive(Clone, Debug, Default, PartialEq)]
struct AmbientLight;

impl CustomEffect for AmbientLight {
    const NAME: &'static str = "zunify-ambient-light";

    fn for_each_parameter(&self, _: &mut dyn FnMut(&'static str, EffectParameterValue)) {}

    fn set_parameter(&mut self, _: &str, _: EffectParameterValue) -> bool {
        false
    }

    fn shader() -> Option<ShaderEffectDescription> {
        Some(ShaderEffectDescription::overlay(
            include_str!("shaders/ambient.sksl"),
            include_str!("shaders/ambient.wgsl"),
            BitmapBlendingMode::SourceOver,
        ))
    }
}

pub(crate) fn light(host: &Border) {
    host.set_effect(Some(ImmutableCustomEffect::new(AmbientLight).into()));
}

#[derive(Clone, Debug, Default, PartialEq)]
struct FireFrame {
    phase: f64,
    bass: f64,
    midrange: f64,
    treble: f64,
}

impl CustomEffect for FireFrame {
    const NAME: &'static str = "zunify-transport-fire";

    fn for_each_parameter(&self, visit: &mut dyn FnMut(&'static str, EffectParameterValue)) {
        visit("phase", self.phase.into());
        visit("bass", self.bass.into());
        visit("midrange", self.midrange.into());
        visit("treble", self.treble.into());
    }

    fn set_parameter(&mut self, name: &str, value: EffectParameterValue) -> bool {
        let Some(value) = value.as_float() else {
            return false;
        };
        match name {
            "phase" => self.phase = value,
            "bass" => self.bass = value,
            "midrange" => self.midrange = value,
            "treble" => self.treble = value,
            _ => return false,
        }
        true
    }

    fn shader() -> Option<ShaderEffectDescription> {
        Some(ShaderEffectDescription::overlay(
            include_str!("shaders/fire.sksl"),
            include_str!("shaders/fire.wgsl"),
            BitmapBlendingMode::SourceOver,
        ))
    }
}

pub(crate) struct TransportFire {
    host: WeakRef<Border>,
    reduced_motion: bool,
    playing: Cell<bool>,
    queued: Cell<bool>,
    last_frame: Cell<f64>,
    track: Cell<Option<usize>>,
    position: Cell<f64>,
    position_at: Cell<f64>,
}

impl TransportFire {
    pub(crate) fn new(host: &Ref<Border>, reduced_motion: bool) -> Rc<Self> {
        Rc::new(Self {
            host: host.downgrade(),
            reduced_motion,
            playing: Cell::new(false),
            queued: Cell::new(false),
            last_frame: Cell::new(f64::NEG_INFINITY),
            track: Cell::new(None),
            position: Cell::new(0.0),
            position_at: Cell::new(0.0),
        })
    }

    pub(crate) fn set_track(&self, id: &str) {
        self.track.set(
            id.strip_prefix("demo:")
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| index.checked_sub(1))
                .filter(|index| *index < 6),
        );
    }

    pub(crate) fn set_position(&self, position: f64) {
        self.position.set(position);
        self.position_at.set(self.last_frame.get());
    }

    pub(crate) fn set_playing(self: &Rc<Self>, playing: bool) {
        let active = playing && self.track.get().is_some() && !self.reduced_motion;
        if self.playing.replace(active) == active {
            return;
        }
        let Some(host) = self.host.upgrade() else {
            return;
        };
        if active {
            self.schedule();
        } else {
            host.set_effect(None);
        }
    }

    fn paint(host: &Border, track: usize, position: f64) {
        // The 20 Hz analysis is baked from the exact bundled audio samples.
        const FRAMES: usize = 540;
        const BANDS: usize = 3;
        const SPECTRUM: &[u8; 6 * FRAMES * BANDS] = include_bytes!("shaders/demo-spectrum.bin");
        let cursor = (position * 20.0).clamp(0.0, (FRAMES - 1) as f64);
        let frame = cursor as usize;
        let start = (track * FRAMES + frame) * BANDS;
        let next = (track * FRAMES + (frame + 1).min(FRAMES - 1)) * BANDS;
        let band = |index| {
            let previous = f64::from(SPECTRUM[start + index]);
            let following = f64::from(SPECTRUM[next + index]);
            (previous + (following - previous) * cursor.fract()) / 255.0
        };
        host.set_effect(Some(
            ImmutableCustomEffect::new(FireFrame {
                phase: position,
                bass: band(0),
                midrange: band(1),
                treble: band(2),
            })
            .into(),
        ));
    }

    fn schedule(self: &Rc<Self>) {
        if self.queued.get() {
            return;
        }
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let Some(top) = TopLevel::get_top_level(Some(&host)) else {
            return;
        };
        self.queued.set(true);
        let fire = Rc::downgrade(self);
        top.request_animation_frame(move |time| {
            let Some(fire) = fire.upgrade() else { return };
            fire.queued.set(false);
            if !fire.playing.get() {
                return;
            }
            let Some(host) = fire.host.upgrade() else {
                return;
            };
            let now = time.total_seconds();
            if now - fire.last_frame.get() >= 1.0 / 30.0 - 0.001 {
                let elapsed = (now - fire.position_at.get()).clamp(0.0, 0.3);
                if let Some(track) = fire.track.get() {
                    Self::paint(&host, track, fire.position.get() + elapsed);
                }
                fire.last_frame.set(now);
            }
            fire.schedule();
        });
    }
}

pub(crate) fn entrance(host: &Ref<Border>, distance: f32, delay_ms: u64) {
    let weak = host.downgrade();
    host.attached_to_visual_tree(move |_| {
        let weak = weak.clone();
        Dispatcher::ui_thread().post_local(
            move || {
                let Some(host) = weak.upgrade() else { return };
                let Some(visual) = ElementCompositionPreview::get_element_visual(&host) else {
                    return;
                };
                let compositor = visual.compositor();
                let duration = Duration::from_millis(300);
                let delay = Duration::from_millis(delay_ms);
                let opacity = compositor.create_scalar_key_frame_animation();
                opacity.insert_key_frame(0.0, 0.0);
                opacity.insert_key_frame(1.0, 1.0);
                opacity.set_duration(duration);
                opacity.set_delay_time(delay);
                opacity.set_delay_behavior(AnimationDelayBehavior::SetInitialValueBeforeDelay);
                visual.start_animation("Opacity", &*opacity);

                let translation = compositor.create_vector3_key_frame_animation();
                translation.insert_key_frame(0.0, Vector3::new(distance, 0.0, 0.0));
                translation.insert_key_frame(1.0, Vector3::default());
                translation.set_duration(duration);
                translation.set_delay_time(delay);
                translation.set_delay_behavior(AnimationDelayBehavior::SetInitialValueBeforeDelay);
                visual.start_animation("Translation", &*translation);
            },
            DispatcherPriority::NORMAL,
        );
    })
    .detach();
}
