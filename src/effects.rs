use crate::spectrum;
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
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

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

pub(crate) fn reveal_light(region: &Grid, backdrop: &Ref<Border>) {
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
}

// FerroUI transports each Rect as a float4 with no colour conversion.
const BAND_GROUPS: [&str; 16] = [
    "bands0", "bands1", "bands2", "bands3", "bands4", "bands5", "bands6", "bands7", "bands8",
    "bands9", "bands10", "bands11", "bands12", "bands13", "bands14", "bands15",
];
const _: () = assert!(BAND_GROUPS.len() * 4 == spectrum::BANDS);

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

/// The fire beneath the transport, shaped by the live spectrum of the audio the app is playing.
/// When music plays that the app cannot hear, the fire settles into a slow glow of its own.
pub(crate) struct TransportFire {
    host: WeakRef<Border>,
    reduced_motion: bool,
    playing: Cell<bool>,
    queued: Cell<bool>,
    lit: Cell<bool>,
    last_frame: Cell<f64>,
    // When the analyser last heard anything, on the animation clock.
    heard_at: Cell<f64>,
    levels: RefCell<[f64; spectrum::BANDS]>,
}

impl TransportFire {
    pub(crate) fn new(host: &Ref<Border>, reduced_motion: bool) -> Rc<Self> {
        Rc::new(Self {
            host: host.downgrade(),
            reduced_motion,
            playing: Cell::new(false),
            queued: Cell::new(false),
            lit: Cell::new(false),
            last_frame: Cell::new(f64::NEG_INFINITY),
            heard_at: Cell::new(f64::NAN),
            levels: RefCell::new([0.0; spectrum::BANDS]),
        })
    }

    pub(crate) fn set_playing(self: &Rc<Self>, playing: bool) {
        let active = playing && !self.reduced_motion;
        if self.playing.replace(active) == active {
            return;
        }
        if active {
            self.last_frame.set(f64::NEG_INFINITY);
            self.heard_at.set(f64::NAN);
            self.schedule();
        } else {
            self.levels.borrow_mut().fill(0.0);
            self.clear();
        }
    }

    fn clear(&self) {
        if self.lit.replace(false)
            && let Some(host) = self.host.upgrade()
        {
            host.set_effect(None);
        }
    }

    /// A slow glow that owes nothing to the music, strongest in the middle of the strip.
    fn ambient(band: usize, now: f64) -> f64 {
        let x = band as f64;
        let drift = (now * 0.9 + x * 0.21).sin() * 0.5
            + (now * 1.7 - x * 0.47).sin() * 0.3
            + (now * 3.1 + x * 1.3).sin() * 0.2;
        let reach = 1.0 - 0.55 * x / (spectrum::BANDS - 1) as f64;
        (0.3 + 0.16 * drift) * reach
    }

    fn paint(&self, host: &Border, now: f64) {
        // How long real silence may last before the fire stops waiting for the music.
        const PATIENCE: f64 = 2.0;
        let heard = spectrum::latest();
        if self.heard_at.get().is_nan()
            || heard.is_some_and(|bands| bands.iter().any(|level| *level > 0.02))
        {
            self.heard_at.set(now);
        }
        // Music on another device, or audio the app may not capture, never reaches the analyser.
        let unheard = now - self.heard_at.get() > PATIENCE;
        let mut levels = self.levels.borrow_mut();
        for (index, level) in levels.iter_mut().enumerate() {
            if unheard {
                *level += (Self::ambient(index, now) - *level) * 0.08;
                continue;
            }
            let target = heard.map_or(0.0, |bands| f64::from(bands[index]));
            // Flames leap with the music and settle more slowly.
            *level += (target - *level) * if target > *level { 0.7 } else { 0.28 };
        }
        if levels.iter().all(|level| *level < 0.004) {
            drop(levels);
            self.clear();
            return;
        }
        let power = levels.iter().map(|value| value * value).sum::<f64>().sqrt() * 0.56;
        let bands = std::array::from_fn(|group| {
            let i = group * 4;
            Rect::new(levels[i], levels[i + 1], levels[i + 2], levels[i + 3])
        });
        self.lit.set(true);
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
                fire.paint(&host, now);
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
