//! Named whole looks and tick spheres over the addressed pane's own bubbles:
//! the window resolves the pane, the pane and its order-flow view serve it.
use crate::bubble_pane::{BubbleLook, PaneBubblesPort, SphereRefusal};
use crate::dock::ActionDock;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control::{error::ControlError, registry::RegistryError};
use quantick_control_schema::bubble_preset::{ApplyInput, ListInput, ResetInput, applied};
use quantick_orderflow::TickTuning;
use serde_json::Value;

const NO_VIEW: &str = "The pane has no order-flow view";

pub fn register<D: ActionDock<H, A>, H: PaneBubblesPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(
        quantick_control_schema::bubble_preset::apply_descriptor(),
        apply,
    )?;
    registry.register(
        quantick_control_schema::bubble_preset::list_descriptor(),
        list,
    )?;
    registry.register(
        quantick_control_schema::bubble_preset::reset_descriptor(),
        reset,
    )
}

fn apply<H: PaneBubblesPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ApplyInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let theme = host.bubble_theme();
    let pane = host.pane_bubbles(input.tab_id, input.pane_id)?;
    // A whole look brings its spheres by the rule tick.set asks.
    let spheres = pane.tick_sphere_pane().may_choose();
    let owner = pane.bubble_look().ok_or_else(|| invalid(NO_VIEW))?;
    let changed = owner
        .wear_preset(&input.preset, spheres, &theme)
        .map_err(invalid)?;
    answer(input.tab_id, input.pane_id, changed, owner)
}

fn reset<H: PaneBubblesPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let pane = host.pane_bubbles(input.tab_id, input.pane_id)?;
    let owner = pane.bubble_look().ok_or_else(|| invalid(NO_VIEW))?;
    let changed = owner.restore_factory_look();
    answer(input.tab_id, input.pane_id, changed, owner)
}

fn list<H: PaneBubblesPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ListInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let pane = host.pane_bubbles(input.tab_id, input.pane_id)?;
    let owner = pane.bubble_look().ok_or_else(|| invalid(NO_VIEW))?;
    serde_json::to_value(owner.presets_page(&input).map_err(invalid)?).map_err(invalid)
}

/// What a whole look answers: whether it moved, and the look it wears now.
fn answer<T>(
    tab_id: WireU64,
    pane_id: WireU64,
    changed: bool,
    owner: &dyn BubbleLook<T>,
) -> Result<Value, ControlError> {
    let snapshot = owner.look_snapshot();
    serde_json::to_value(applied(
        (tab_id, pane_id),
        changed,
        snapshot,
        owner.look_config(),
    ))
    .map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The chart-only selector: the rule reads the pane's facts, and the pane's
/// view wears what it admitted. The policy stays here.
pub fn register_tick<D: ActionDock<H, A>, H: PaneBubblesPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::tick_bubbles::descriptor(), tick)
}
fn tick<H: PaneBubblesPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: quantick_control_schema::tick_bubbles::TickBubblesInput =
        serde_json::from_value(input.clone()).map_err(invalid)?;
    let preset = input.preset.as_deref();
    let tuning = tick_tuning(&input)?;
    let pane = host.pane_bubbles(input.tab_id, input.pane_id)?;
    pane.tick_sphere_pane().admit(preset)?;
    let owner = pane.bubble_look().ok_or_else(|| invalid(NO_VIEW))?;
    let changed = owner.wear_tick(preset, tuning).map_err(invalid)?;
    let worn = owner.look_config().volume_dots.tick_bubbles;
    serde_json::to_value(quantick_control_schema::tick_bubbles::TickBubblesResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        changed,
        tick_bubbles: worn.map(Into::into),
    })
    .map_err(invalid)
}

/// The tuning `input` asks of its preset, validated before any edit: an
/// opacity the panel's slider could set, and none without a preset.
fn tick_tuning(
    input: &quantick_control_schema::tick_bubbles::TickBubblesInput,
) -> Result<TickTuning, ControlError> {
    use quantick_orderflow::config::controls::OPACITY_RANGE;
    let opacity = input
        .opacity
        .as_ref()
        .map(|value| {
            value
                .as_str()
                .parse::<f32>()
                .ok()
                .filter(|opacity| OPACITY_RANGE.contains(opacity))
                .ok_or_else(|| {
                    invalid(format!(
                        "opacity must be a decimal from {} to {}",
                        OPACITY_RANGE.start(),
                        OPACITY_RANGE.end()
                    ))
                })
        })
        .transpose()?;
    let tuning = TickTuning {
        peak_spheres: input.peak_spheres,
        opacity,
    };
    if input.preset.is_none() && tuning != TickTuning::default() {
        return Err(invalid(SphereRefusal::NoNamedPreset.reason()));
    }
    Ok(tuning)
}

#[cfg(test)]
#[path = "bubble_preset_tests.rs"]
mod tests;
