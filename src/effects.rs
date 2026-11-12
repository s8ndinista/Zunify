use crate::{spectrum, view::mount};
use ferroui_base::{
    Rect, Ref, Visual, WeakRef,
    animation::{DoubleTransition, ITransition, Transitions},
    input::{InputElement, PointerType},
    interactivity::RoutingStrategies,
    layout::{HorizontalAlignment, VerticalAlignment},
    media::{
        Color, GradientStop, RadialGradientBrush, ScaleTransform, TranslateTransform,
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
use ferroui_view::prelude::{InputElementProps, LayoutableProps, VisualProps, border};
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

pub(crate) fn reveal_light(region: &Grid, backdrop: &Ref<Border>) -> WeakRef<Border> {
    const WIDTH: f64 = 350.0;
    const HEIGHT: f64 = 300.0;
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
    let glow = mount(
        border()
            .width(WIDTH)
            .height(HEIGHT)
            .horizontal_alignment(HorizontalAlignment::Left)
            .vertical_alignment(VerticalAlignment::Top)
            .is_hit_test_visible(false)
            .opacity(0.0)
            .with(move |glow| glow.set_background(Some(falloff.clone().into()))),
    );

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
/// Silence lets the flames fall away. Only when the app has no way to listen to the music does the
/// fire move at random, out of time with it.
pub(crate) struct TransportFire {
    host: WeakRef<Border>,
    reduced_motion: bool,
    playing: Cell<bool>,
    queued: Cell<bool>,
    lit: Cell<bool>,
    last_frame: Cell<f64>,
    levels: RefCell<[f64; spectrum::BANDS]>,
    meter: RefCell<Option<[Ref<ScaleTransform>; 4]>>,
    // The invented spectrum shown when the app cannot listen.
    targets: RefCell<[f64; spectrum::BANDS]>,
    next_kick: Cell<f64>,
    seed: Cell<u32>,
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
            levels: RefCell::new([0.0; spectrum::BANDS]),
            meter: RefCell::new(None),
            targets: RefCell::new([0.0; spectrum::BANDS]),
            next_kick: Cell::new(0.0),
            seed: Cell::new(0x9E37_79B9),
        })
    }

    pub(crate) fn set_playing(self: &Rc<Self>, playing: bool) {
        let active = playing && !self.reduced_motion;
        if self.playing.replace(active) == active {
            return;
        }
        if active {
            self.last_frame.set(f64::NEG_INFINITY);
            self.schedule();
        }
        // When the music stops, the frames keep coming until the flames have died down.
    }

    pub(crate) fn set_meter(&self, bars: [Ref<ScaleTransform>; 4]) {
        self.meter.replace(Some(bars));
        self.paint_meter(&self.levels.borrow());
    }

    fn paint_meter(&self, levels: &[f64; spectrum::BANDS]) {
        if let Some(bars) = self.meter.borrow().as_ref() {
            for ((bar, bands), floor) in bars.iter().zip(levels.chunks_exact(spectrum::BANDS / 4))
                .zip([0.20, 0.39, 0.54, 0.27])
            {
                let peak = bands.iter().copied().fold(0.0, f64::max);
                bar.set_scale_y(floor + (1.0 - floor) * (peak * 1.4).min(1.0));
            }
        }
    }

    fn clear(&self) {
        if self.lit.replace(false)
            && let Some(host) = self.host.upgrade()
        {
            host.set_effect(None);
        }
    }

    fn random(&self) -> f64 {
        // xorshift32
        let mut x = self.seed.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed.set(x);
        f64::from(x) / f64::from(u32::MAX)
    }

    /// Invents a restless spectrum for music the app cannot hear. It keeps no beat: kicks land
    /// in the bass at uneven intervals while the other bands jump about and settle.
    fn improvise(&self, now: f64) {
        let mut targets = self.targets.borrow_mut();
        for target in targets.iter_mut() {
            *target *= 0.86;
        }
        if now >= self.next_kick.get() {
            self.next_kick.set(now + 0.22 + 0.5 * self.random());
            let strength = 0.55 + 0.4 * self.random();
            for (band, target) in targets.iter_mut().take(14).enumerate() {
                *target = target.max(strength * (1.0 - band as f64 / 20.0));
            }
        }
        // A few bands leap each frame, the quieter treble less often and less far.
        for _ in 0..5 {
            let band = (self.random() * self.random() * spectrum::BANDS as f64) as usize;
            let band = band.min(spectrum::BANDS - 1);
            let reach = 1.0 - 0.6 * band as f64 / (spectrum::BANDS - 1) as f64;
            let leap = (0.25 + 0.6 * self.random()) * reach;
            for (offset, spread) in [(0usize, 1.0), (1, 0.6), (2, 0.3)] {
                if let Some(target) = targets.get_mut(band + offset) {
                    *target = target.max(leap * spread);
                }
            }
        }
    }

    fn paint(&self, host: &Border, now: f64) {
        let playing = self.playing.get();
        let heard = if playing { spectrum::latest() } else { None };
        let glowing = playing && !spectrum::listening();
        if glowing {
            self.improvise(now);
        }
        let targets = self.targets.borrow();
        let mut levels = self.levels.borrow_mut();
        for (index, level) in levels.iter_mut().enumerate() {
            let target = if glowing {
                targets[index]
            } else {
                heard.map_or(0.0, |bands| f64::from(bands[index]))
            };
            // Flames leap with the music and fall away more slowly.
            *level += (target - *level) * if target > *level { 0.7 } else { 0.2 };
        }
        self.paint_meter(&levels);
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
            if !fire.playing.get() && !fire.lit.get() {
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
    fade_entrance(host, Duration::from_millis(300), distance, delay_ms);
}

pub(crate) fn fade_in(host: &Ref<Border>, duration: Duration) {
    fade_entrance(host, duration, 0.0, 0);
}

fn fade_entrance(host: &Ref<Border>, duration: Duration, distance: f32, delay_ms: u64) {
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
                let delay = Duration::from_millis(delay_ms);
                let opacity = compositor.create_scalar_key_frame_animation();
                opacity.insert_key_frame(0.0, 0.0);
                opacity.insert_key_frame(1.0, 1.0);
                opacity.set_duration(duration);
                opacity.set_delay_time(delay);
                opacity.set_delay_behavior(AnimationDelayBehavior::SetInitialValueBeforeDelay);
                visual.start_animation("Opacity", &*opacity);

                if distance != 0.0 {
                    let translation = compositor.create_vector3_key_frame_animation();
                    translation.insert_key_frame(0.0, Vector3::new(distance, 0.0, 0.0));
                    translation.insert_key_frame(1.0, Vector3::default());
                    translation.set_duration(duration);
                    translation.set_delay_time(delay);
                    translation.set_delay_behavior(AnimationDelayBehavior::SetInitialValueBeforeDelay);
                    visual.start_animation("Translation", &*translation);
                }
            },
            DispatcherPriority::NORMAL,
        );
    })
    .detach();
}
