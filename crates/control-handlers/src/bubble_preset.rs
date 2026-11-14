//! Named whole looks over the pane owner's narrow port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_preset::{
    ApplyInput, ApplyResult, ListInput, ListResult, ResetInput,
};
use quantick_orderflow::TickTuning;
use serde_json::Value;

/// What wearing a whole preset answers.
pub type PresetApplied = Result<ApplyResult, ControlError>;

pub trait BubblePresetPort {
    /// Wear the whole preset `input` names; its tick spheres only when
    /// `spheres`, the pane's [`TickSpherePane::may_choose`].
    fn apply_bubble_preset(&mut self, input: &ApplyInput, spheres: bool) -> PresetApplied;
    fn list_bubble_presets(&mut self, input: &ListInput) -> Result<ListResult, ControlError>;
    /// Restore the pane's factory look, as the panel's reset button does:
    /// the look a fresh open shows, its tick spheres included.
    fn reset_bubbles(&mut self, input: &ResetInput) -> PresetApplied;
}

pub fn register<D: ActionDock<H, A>, H: BubblePresetPort + TickBubblePort + ?Sized, A>(
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

fn apply<H: BubblePresetPort + TickBubblePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ApplyInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    // A whole look brings its spheres by the rule tick.set asks.
    let spheres = host
        .tick_sphere_pane(input.tab_id.get(), input.pane_id.get())?
        .may_choose();
    serde_json::to_value(host.apply_bubble_preset(&input, spheres)?).map_err(invalid)
}

fn reset<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.reset_bubbles(&input)?).map_err(invalid)
}

fn list<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.list_bubble_presets(&input)?).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// What the tick sphere rule reads about a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickSpherePane {
    /// The pane draws tick bars.
    pub tick_chart: bool,
    /// The config switches the mode on for the pane's asset
    /// (`symbol_asset_modes` in feeds.toml).
    pub asset_mode: bool,
}

impl TickSpherePane {
    #[must_use]
    pub const fn new(tick_chart: bool, asset_mode: bool) -> Self {
        Self {
            tick_chart,
            asset_mode,
        }
    }

    /// The one rule for tick spheres, which the settings picker and
    /// `orderflow.bubbles.tick.set` both ask: they may be chosen on a tick
    /// chart whose asset the config switches the mode on for.
    #[must_use]
    pub fn may_choose(self) -> bool {
        self.tick_chart && self.asset_mode
    }

    /// What the settings picker offers, by the same rule: the asset's
    /// `presets` where spheres may be chosen; only the switch-off where they
    /// may not but the pane is `wearing` some; no picker otherwise.
    #[must_use]
    pub fn offered(self, presets: Vec<String>, wearing: bool) -> Option<Vec<String>> {
        if self.may_choose() {
            Some(presets)
        } else {
            wearing.then(Vec::new)
        }
    }

    /// Whether the spheres a pane wears may be tuned (peak spheres, opacity),
    /// from the settings panel or `orderflow.bubbles.tick.set`. The call tunes
    /// a named preset, so the panel may tune exactly when the pane wears one
    /// the call would admit: [`Self::admit`] of `worn`, and a pane wearing
    /// custom spheres or none has nothing the call could name.
    ///
    /// # Errors
    ///
    /// Names why tuning is refused, in the words the call refuses with.
    pub fn tune(self, worn: Option<&str>) -> Result<(), SphereRefusal> {
        let worn = worn.ok_or(SphereRefusal::NoNamedPreset)?;
        self.admit(Some(worn))
    }

    /// Whether `preset` may be worn. Dropping spheres always may, whatever
    /// put them on (review #29); a named preset needs [`Self::may_choose`].
    ///
    /// # Errors
    ///
    /// Names which half of the rule a named preset fails.
    pub fn admit(self, preset: Option<&str>) -> Result<(), SphereRefusal> {
        match preset {
            None => Ok(()),
            Some(_) if !self.tick_chart => Err(SphereRefusal::NotTickChart),
            Some(_) if !self.asset_mode => Err(SphereRefusal::AssetModeOff),
            Some(_) => Ok(()),
        }
    }
}

/// Which half of the tick sphere rule a named preset fails, so a caller
/// branches on the reason instead of parsing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphereRefusal {
    /// The pane does not draw tick bars.
    NotTickChart,
    /// The feed config switches no asset mode on for the pane's asset.
    AssetModeOff,
    /// The pane wears custom spheres or none: there is no preset to tune.
    NoNamedPreset,
}

impl SphereRefusal {
    /// Why, in the words `orderflow.bubbles.tick.set` refuses with.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::NotTickChart => "Tick sphere presets require a tick chart",
            Self::AssetModeOff => {
                "Tick sphere presets require an asset whose config asset mode is on"
            }
            Self::NoNamedPreset => "peak_spheres and opacity tune a named preset",
        }
    }
}

impl From<SphereRefusal> for ControlError {
    fn from(refusal: SphereRefusal) -> Self {
        use quantick_control_schema::{bubble_scale, tick_bubbles};
        match refusal {
            SphereRefusal::NotTickChart => crate::bubble_scale::refused_precondition(
                bubble_scale::TICK_CHART_PRECONDITION,
                refusal.reason(),
                "switch the pane to a tick chart, or send a null preset",
            ),
            SphereRefusal::AssetModeOff => crate::bubble_scale::refused_precondition(
                tick_bubbles::ASSET_MODE_PRECONDITION,
                refusal.reason(),
                "switch an asset mode on for this asset in the feed config (symbol_asset_modes)",
            ),
            SphereRefusal::NoNamedPreset => invalid(refusal.reason()),
        }
    }
}

/// What wearing tick spheres answers: whether the look changed, and the
/// spheres it wears after.
pub type WornSpheres = Result<
    (
        bool,
        Option<quantick_control_schema::tick_bubbles::TickBubblesSnapshot>,
    ),
    ControlError,
>;

/// The chart-only selector's narrow port: the facts the rule reads, and
/// wearing what the rule admitted. The policy stays here.
pub trait TickBubblePort {
    /// The addressed pane's facts for [`TickSpherePane::admit`].
    fn tick_sphere_pane(
        &mut self,
        tab_id: u64,
        pane_id: u64,
    ) -> Result<TickSpherePane, ControlError>;
    /// Wear `preset`'s spheres with `tuning` applied, or none.
    fn wear_tick_spheres(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        preset: Option<&str>,
        tuning: TickTuning,
    ) -> WornSpheres;
}
pub fn register_tick<D: ActionDock<H, A>, H: TickBubblePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::tick_bubbles::descriptor(), tick)
}
fn tick<H: TickBubblePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: quantick_control_schema::tick_bubbles::TickBubblesInput =
        serde_json::from_value(input.clone()).map_err(invalid)?;
    let (tab, pane, preset) = (
        input.tab_id.get(),
        input.pane_id.get(),
        input.preset.as_deref(),
    );
    let tuning = tick_tuning(&input)?;
    host.tick_sphere_pane(tab, pane)?.admit(preset)?;
    let (changed, tick_bubbles) = host.wear_tick_spheres(tab, pane, preset, tuning)?;
    serde_json::to_value(quantick_control_schema::tick_bubbles::TickBubblesResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        changed,
        tick_bubbles,
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
mod tick_rule_tests {
    use super::*;
    use serde_json::json;

    /// Review #19/#29: one rule decides tick spheres for the picker and for
    /// tick.set. Choosing needs a tick chart and the asset's config mode;
    /// dropping them is always allowed, whatever put them on.
    /// Review round 2 #3: the panel tunes exactly when the call could: a
    /// named preset worn where the call admits it.
    #[test]
    fn tuning_follows_the_rule_the_call_asks() {
        let pane = |tick_chart, asset_mode| TickSpherePane {
            tick_chart,
            asset_mode,
        };
        assert_eq!(pane(true, true).tune(Some("mini index regions")), Ok(()));
        assert_eq!(
            pane(true, true).tune(None),
            Err(SphereRefusal::NoNamedPreset)
        );
        assert_eq!(
            pane(false, true).tune(Some("mini index regions")),
            Err(SphereRefusal::NotTickChart)
        );
        assert_eq!(
            pane(true, false).tune(Some("mini index regions")),
            Err(SphereRefusal::AssetModeOff)
        );
        for refusal in [SphereRefusal::NotTickChart, SphereRefusal::AssetModeOff] {
            let error = ControlError::from(refusal);
            assert!(format!("{error:?}").contains(refusal.reason()), "{error:?}");
        }
    }

    #[test]
    fn tick_spheres_need_a_tick_chart_and_the_asset_mode_but_anyone_may_drop_them() {
        for tick_chart in [false, true] {
            for asset_mode in [false, true] {
                let pane = TickSpherePane {
                    tick_chart,
                    asset_mode,
                };
                assert_eq!(pane.may_choose(), tick_chart && asset_mode);
                assert_eq!(
                    pane.admit(Some("Binance BTC Spot spheres")).is_ok(),
                    tick_chart && asset_mode,
                    "{pane:?}"
                );
                assert!(pane.admit(None).is_ok(), "{pane:?}");
                let presets = vec!["Binance BTC Spot spheres".to_owned()];
                let offered = pane.offered(presets.clone(), false);
                assert_eq!(offered, pane.may_choose().then(|| presets.clone()));
                let wearing = pane.offered(presets.clone(), true);
                assert_eq!(
                    wearing,
                    Some(if pane.may_choose() {
                        presets
                    } else {
                        Vec::new()
                    }),
                    "spheres a pane wears can always be switched off"
                );
            }
        }
    }

    struct Pane {
        facts: TickSpherePane,
        worn: Option<Option<String>>,
        tuning: TickTuning,
    }
    impl TickBubblePort for Pane {
        fn tick_sphere_pane(
            &mut self,
            _tab_id: u64,
            _pane_id: u64,
        ) -> Result<TickSpherePane, ControlError> {
            Ok(self.facts)
        }
        fn wear_tick_spheres(
            &mut self,
            _tab_id: u64,
            _pane_id: u64,
            preset: Option<&str>,
            tuning: TickTuning,
        ) -> WornSpheres {
            self.worn = Some(preset.map(str::to_owned));
            self.tuning = tuning;
            Ok((true, None))
        }
    }

    #[test]
    fn tick_set_asks_the_rule_before_the_pane_wears_anything() {
        let input = |preset: Value| json!({"tab_id": "1", "pane_id": "2", "preset": preset});
        let actor = crate::test_support::FakeWindow::assistant();
        let mut volume_chart = Pane {
            facts: TickSpherePane {
                tick_chart: false,
                asset_mode: true,
            },
            worn: None,
            tuning: TickTuning::default(),
        };
        let refused = tick(
            &mut volume_chart,
            &mut (),
            &actor,
            &input(json!("Binance BTC Spot spheres")),
        );
        let violated = |error: ControlError| {
            assert_eq!(error.context.next_steps.len(), 1, "{error:?}");
            error
                .context
                .violated_precondition
                .map(|id| id.as_str().to_owned())
        };
        assert_eq!(
            violated(refused.unwrap_err()).as_deref(),
            Some(quantick_control_schema::bubble_scale::TICK_CHART_PRECONDITION)
        );
        assert_eq!(volume_chart.worn, None, "refused before any edit");
        let mut no_mode = Pane {
            facts: TickSpherePane {
                tick_chart: true,
                asset_mode: false,
            },
            worn: None,
            tuning: TickTuning::default(),
        };
        let refused = tick(
            &mut no_mode,
            &mut (),
            &actor,
            &input(json!("Binance BTC Spot spheres")),
        );
        assert_eq!(
            violated(refused.unwrap_err()).as_deref(),
            Some(quantick_control_schema::tick_bubbles::ASSET_MODE_PRECONDITION)
        );
        assert_eq!(no_mode.worn, None, "refused before any edit");
        let mut plain_asset = Pane {
            facts: TickSpherePane {
                tick_chart: true,
                asset_mode: false,
            },
            worn: None,
            tuning: TickTuning::default(),
        };
        let dropped = tick(&mut plain_asset, &mut (), &actor, &input(Value::Null)).unwrap();
        assert_eq!(
            plain_asset.worn,
            Some(None),
            "spheres can always be dropped"
        );
        assert_eq!(dropped["changed"], true);
        let mut btc = Pane {
            facts: TickSpherePane {
                tick_chart: true,
                asset_mode: true,
            },
            worn: None,
            tuning: TickTuning::default(),
        };
        tick(
            &mut btc,
            &mut (),
            &actor,
            &input(json!("Binance BTC Spot spheres")),
        )
        .unwrap();
        assert_eq!(btc.worn, Some(Some("Binance BTC Spot spheres".to_owned())));
        assert_eq!(btc.tuning, TickTuning::default());
        let tuned = |extra: Value| {
            let mut request = input(json!("mini index regions"));
            for (key, value) in extra.as_object().unwrap() {
                request[key] = value.clone();
            }
            request
        };
        tick(
            &mut btc,
            &mut (),
            &actor,
            &tuned(json!({"peak_spheres": false, "opacity": "0.6"})),
        )
        .unwrap();
        assert_eq!(
            btc.tuning,
            TickTuning {
                peak_spheres: Some(false),
                opacity: Some(0.6),
            }
        );
        btc.worn = None;
        for refused in [
            json!({"opacity": "1.5"}),
            json!({"opacity": "0"}),
            json!({"opacity": 0.5}),
            json!({"peak_spheres": "yes"}),
        ] {
            let error = tick(&mut btc, &mut (), &actor, &tuned(refused.clone()));
            assert!(error.is_err(), "{refused}");
        }
        let mut strip = input(Value::Null);
        strip["peak_spheres"] = json!(true);
        assert!(tick(&mut btc, &mut (), &actor, &strip).is_err());
        assert_eq!(btc.worn, None, "every refusal comes before any edit");
    }
}
