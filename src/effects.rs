use ferroui_base::{
    Rect, Ref, Visual, WeakRef,
    animation::{DoubleTransition, ITransition, Transitions},
    input::{InputElement, PointerType},
    interactivity::RoutingStrategies,
    layout::{HorizontalAlignment, VerticalAlignment},
    media::{
        Color, GradientStop, RadialGradientBrush, TranslateTransform,
        effects::{
            CustomEffect, EffectParameterValue, ImmutableCustomEffect, ShaderEffectDescription,
        },
        imaging::BitmapBlendingMode,
    },
    numerics::Vector3,
    rendering::composition::{ICompositionObjectAnimations, animations::AnimationDelayBehavior},
    threading::{Dispatcher, DispatcherPriority},
};
use ferroui_controls::{Border, Grid, TopLevel, hosting::ElementCompositionPreview};
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

pub(crate) fn reveal_light(region: &Grid, backdrop: &Ref<Border>) -> WeakRef<Border> {
    const WIDTH: f64 = 350.0;
    const HEIGHT: f64 = 300.0;
    let glow = Border::new();
    glow.set_width(WIDTH);
    glow.set_height(HEIGHT);
    glow.set_horizontal_alignment(HorizontalAlignment::Left);
    glow.set_vertical_alignment(VerticalAlignment::Top);
    glow.set_is_hit_test_visible(false);
    glow.set_opacity(0.0);
    let falloff = RadialGradientBrush::new();
    for (offset, alpha, r, g, b) in [
        (0.0, 42, 219, 37, 132),
        (0.48, 18, 246, 59, 153),
        (1.0, 0, 246, 59, 153),
    ] {
        falloff
            .gradient_stops()
            .add(GradientStop::with_color_and_offset(
                Color::from_argb(alpha, r, g, b),
                offset,
            ));
    }
    glow.set_background(Some(falloff.into()));

    let shift = TranslateTransform::new();
    glow.set_render_transform(Some(shift.clone().into()));
    let fade = DoubleTransition::new();
    fade.set_property(Some(Visual::opacity_property().as_property()));
    fade.set_duration(Duration::from_millis(160).into());
    let fade: Rc<dyn ITransition> = fade.into();
    glow.set_transitions(Some(Transitions::from_items([fade])));
    backdrop.set_child(glow.clone());

    let target = glow.downgrade();
    let space = backdrop.downgrade();
    region.add_handler_with(
        &InputElement::pointer_moved_event(),
        move |_, event| {
            if event.pointer().type_() != PointerType::Touch
                && let (Some(glow), Some(space)) = (target.upgrade(), space.upgrade())
            {
                let at = event.get_position(Some(&space));
                shift.set_x(at.x - WIDTH / 2.0);
                shift.set_y(at.y - HEIGHT / 2.0);
                glow.set_opacity(1.0);
            }
        },
        RoutingStrategies::TUNNEL,
        true,
    );
    let target = glow.downgrade();
    region.add_handler(InputElement::pointer_exited_event(), move |_, _| {
        if let Some(glow) = target.upgrade() {
            glow.set_opacity(0.0);
        }
    });
    glow.downgrade()
}

// FerroUI transports each Rect as a float4 with no colour conversion.
const BAND_GROUPS: [&str; 16] = [
    "bands0", "bands1", "bands2", "bands3", "bands4", "bands5", "bands6", "bands7", "bands8",
    "bands9", "bands10", "bands11", "bands12", "bands13", "bands14", "bands15",
];
const SPECTRUM_BANDS: usize = BAND_GROUPS.len() * 4;

#[derive(Clone, Debug, Default, PartialEq)]
struct FireFrame {
    power: f64,
    bands: [Rect; BAND_GROUPS.len()],
}

impl CustomEffect for FireFrame {
    const NAME: &'static str = "zunify-transport-fire";

    fn for_each_parameter(&self, visit: &mut dyn FnMut(&'static str, EffectParameterValue)) {
        visit("power", self.power.into());
        for (name, bands) in BAND_GROUPS.into_iter().zip(self.bands) {
            visit(name, bands.into());
        }
    }

    fn set_parameter(&mut self, name: &str, value: EffectParameterValue) -> bool {
        if name == "power"
            && let Some(power) = value.as_float()
        {
            self.power = power;
            return true;
        }
        if let Some(index) = BAND_GROUPS.iter().position(|group| *group == name)
            && let Some(bands) = value.as_rect()
        {
            self.bands[index] = bands;
            return true;
        }
        false
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
            self.last_frame.set(f64::NEG_INFINITY);
            self.position_at.set(f64::NEG_INFINITY);
            self.schedule();
        } else {
            host.set_effect(None);
        }
    }

    fn paint(host: &Border, track: usize, position: f64) {
        // The 20 Hz analysis is baked from the exact bundled audio samples.
        const FRAMES: usize = 540;
        const SPECTRUM: &[u8; 6 * FRAMES * SPECTRUM_BANDS] =
            include_bytes!("shaders/demo-spectrum.bin");
        let cursor = (position * 20.0).clamp(0.0, (FRAMES - 1) as f64);
        let frame = cursor as usize;
        let start = (track * FRAMES + frame) * SPECTRUM_BANDS;
        let next = (track * FRAMES + (frame + 1).min(FRAMES - 1)) * SPECTRUM_BANDS;
        let band = |index| {
            let previous = f64::from(SPECTRUM[start + index]);
            let following = f64::from(SPECTRUM[next + index]);
            (previous + (following - previous) * cursor.fract()) / 255.0
        };
        let magnitudes: [f64; SPECTRUM_BANDS] = std::array::from_fn(band);
        let power = magnitudes
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt()
            * 0.56;
        let bands = std::array::from_fn(|group| {
            let i = group * 4;
            Rect::new(
                magnitudes[i],
                magnitudes[i + 1],
                magnitudes[i + 2],
                magnitudes[i + 3],
            )
        });
        host.set_effect(Some(
            ImmutableCustomEffect::new(FireFrame {
                power: power.min(1.0),
                bands,
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
                let anchor = fire.position_at.get();
                let elapsed = if anchor.is_finite() {
                    (now - anchor).clamp(0.0, 0.3)
                } else {
                    fire.position_at.set(now);
                    0.0
                };
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
