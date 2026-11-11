use ferroui_base::{
    Ref,
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
use ferroui_controls::{Border, hosting::ElementCompositionPreview};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AmbientLight {
    pub(crate) transport: bool,
}

impl CustomEffect for AmbientLight {
    const NAME: &'static str = "zunify-ambient-light";

    fn for_each_parameter(&self, visit: &mut dyn FnMut(&'static str, EffectParameterValue)) {
        visit(
            "transport",
            (if self.transport { 1.0_f64 } else { 0.0 }).into(),
        );
    }

    fn set_parameter(&mut self, name: &str, value: EffectParameterValue) -> bool {
        if name == "transport"
            && let Some(value) = value.as_float()
        {
            self.transport = value >= 0.5;
            return true;
        }
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

pub(crate) fn light(host: &Border, transport: bool) {
    host.set_effect(Some(
        ImmutableCustomEffect::new(AmbientLight { transport }).into(),
    ));
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
