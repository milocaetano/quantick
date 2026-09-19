use crate::{InputsRebound, SessionEffects};
use quantick_indicators::{Indicator, InputValue, native::native};

/// What to instantiate behind a slot.
#[derive(Debug, Clone)]
pub enum IndicatorSource {
    /// A native, by its catalog id, with the input values it was restored
    /// with — empty meaning "whatever the native declares".
    ///
    /// One variant for every native there will ever be: the worker resolves
    /// the id against [`quantick_indicators::native`] and never learns which
    /// natives exist.
    Native {
        /// Stable catalog id (`native.ema`).
        id: String,
        /// Saved input values, in binding order.
        values: Vec<InputValue>,
    },
    /// A Quantick Pine script: display name + source text (the UI owns
    /// files; the worker only ever sees text).
    Script { name: String, text: String },
}

/// Saved values that were all readable, in the cell form
/// [`quantick_indicators::bind_by_position`] takes. The preset file can yield
/// a `None` — a stored cell that no longer parses, which must still hold its
/// index — while these came from a live indicator and cannot. Widening, not a
/// loss.
fn to_cells(values: &[InputValue]) -> Vec<Option<InputValue>> {
    values.iter().cloned().map(Some).collect()
}

impl IndicatorSource {
    /// Build the indicator, or explain why the script does not load. The
    /// error string is the full human rendering — every problem, each with
    /// file:line:col and its stable code.
    pub(crate) fn build(
        &self,
        effects: &mut impl SessionEffects,
    ) -> Result<Box<dyn Indicator>, String> {
        self.build_with(None, effects)
    }

    /// Build with bound input values (the settings apply path). `None` =
    /// declared defaults. Values are defensive: a missing or mistyped cell
    /// falls back to its default rather than panicking the worker.
    pub(crate) fn build_with(
        &self,
        values: Option<&[InputValue]>,
        effects: &mut impl SessionEffects,
    ) -> Result<Box<dyn Indicator>, String> {
        match self {
            IndicatorSource::Native { id, values: saved } => {
                // An id this build does not ship is an error slot saying so,
                // never a substitute indicator: silently building a different
                // native than the workspace named is how a trader ends up
                // reading an EMA and believing it is something else.
                let entry = native(id)
                    .ok_or_else(|| format!("`{id}` is not a native indicator this build ships"))?;
                // The panel is generated from `InputSpec` and binding the
                // values back is generated too, via the trait, so a new
                // native's settings cannot be silently ignored for want of a
                // match arm here.
                // An empty set means "never given one" — the same reading
                // the script arm below takes — so it falls back to the values
                // this source was restored with rather than silently
                // resetting a tuned native to its declared defaults.
                let bind = match values {
                    Some(values) if !values.is_empty() => values,
                    _ => saved,
                };
                Ok(entry.build_with(bind))
            }
            IndicatorSource::Script { name, text } => match quantick_pine::compile(text, name) {
                Ok(compiled) => Ok(Box::new(match values {
                    // An empty set is a slot that has never been given one:
                    // a brand-new indicator, or one whose first compile failed
                    // before any `SetInputs` reached it. Nothing to bind and
                    // nothing to report — and note this is now genuinely rare,
                    // because `SetInputs` keeps the values even when it has no
                    // instance to apply them to.
                    Some(values) if !values.is_empty() => {
                        // Cell by cell, type-checked, through the one binder
                        // both persistence paths use — an edited or upgraded
                        // script keeps every setting whose input is still
                        // there, at its type, and only the rest fall back.
                        // Binding the whole vector on an exact count match
                        // instead would take a stale value whenever a script
                        // changed an input's TYPE without changing how many it
                        // has; refusing the whole vector on a count mismatch
                        // (which this did) meant one added knob reset every
                        // other one — "I reopened the app and my settings were
                        // gone".
                        let bound = quantick_indicators::bind_by_position(
                            &compiled.inputs,
                            &to_cells(values),
                        );
                        // Two reasons to speak, and the count is only one of
                        // them: a cell can be refused at an unchanged count
                        // when an input changed type. The other is louder than
                        // it looks — when the list grew or shrank, EVERY value
                        // after the edit may now sit on a different knob, and
                        // that binds silently whenever the types happen to
                        // line up. A count change is therefore worth a line
                        // even when nothing was refused.
                        let count_changed = values.len() != compiled.inputs.len();
                        if count_changed || bound.kept < values.len() {
                            effects.inputs_rebound(InputsRebound {
                                script: name,
                                saved: values.len(),
                                declared: compiled.inputs.len(),
                                kept: bound.kept,
                                count_changed,
                            });
                        }
                        quantick_pine::ScriptIndicator::with_inputs(
                            compiled,
                            text.clone(),
                            bound.values,
                        )
                    }
                    _ => quantick_pine::ScriptIndicator::new(compiled, text.clone()),
                })),
                Err(errors) => Err(errors
                    .iter()
                    .map(|e| e.render(name, text))
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    )),
            },
        }
    }

    /// The constructor this instance was added through, as a stable string.
    ///
    /// It is the durable half of a pane's identity: unlike the slot id (a
    /// monotonic counter, so remove + add always yields a new one) and unlike
    /// the title (which moves with the inputs), this is the same string
    /// before and after the trader takes an indicator off the chart and puts
    /// it back. Drawings anchored to a pane are keyed on it — see
    /// the chart's pane keys.
    ///
    /// Deliberately excludes the input values: changing a period changes the
    /// series, not which pane the trader was annotating.
    pub fn kind_id(&self) -> String {
        match self {
            IndicatorSource::Native { id, .. } => id.clone(),
            IndicatorSource::Script { name, .. } => format!("script.{name}"),
        }
    }

    /// The display title used when the source cannot load (a healthy
    /// instance's title comes from its descriptor).
    pub(crate) fn fallback_title(&self) -> String {
        match self {
            IndicatorSource::Native { id, values } => native(id).map_or_else(
                // Nothing shipped under this id, so the id itself is the most
                // useful thing the error row can say.
                || id.clone(),
                |entry| entry.build_with(values).descriptor().title.clone(),
            ),
            IndicatorSource::Script { name, .. } => name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndicatorEvent;
    use quantick_indicators::SourceId;
    struct Silent;
    impl SessionEffects for Silent {
        fn event(&mut self, _: IndicatorEvent) {}
        fn inputs_rebound(&mut self, _: InputsRebound<'_>) {}
    }
    /// A script with three inputs, the third of which is imagined to have
    /// been added after the trader saved their settings for the first two.
    fn grown_script() -> IndicatorSource {
        IndicatorSource::Script {
            name: "grown.pine".to_owned(),
            text: concat!(
                "//@version=5\n",
                "indicator(\"Grown\", overlay=true)\n",
                "len = input.int(20, \"len\")\n",
                "factor = input.float(1.5, \"factor\")\n",
                "added = input.bool(false, \"added\")\n",
                "plot(close)\n"
            )
            .to_owned(),
        }
    }
    /// Saved indicator settings are stored positionally, so a script that
    /// gained an input arrives with fewer saved values than declared inputs.
    /// Binding only on an exact count match meant the trader lost the
    /// settings for every input that WAS still there — one added knob, and a
    /// whole tuned indicator came back at its defaults.
    #[test]
    fn a_script_that_gained_an_input_keeps_the_values_saved_for_the_older_ones() {
        let saved = vec![InputValue::Int(50), InputValue::Float(2.5)];
        let built = grown_script()
            .build_with(Some(&saved), &mut Silent)
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(2.5),
                InputValue::Bool(false),
            ],
            "the two saved values survive at their own indices and the input added since takes its declared default"
        );
    }
    /// The type check is what keeps the per-cell bind honest: a value that no
    /// longer matches the input at its index is not carried over, it is
    /// dropped for that input's default — and its neighbours are unaffected.
    ///
    /// It applies at every count, including a matching one: there is no
    /// longer a fast path that hands the vector over unchecked, so a script
    /// that changed an input's TYPE without changing how many it has no
    /// longer binds the stale value.
    #[test]
    fn a_saved_value_whose_input_changed_type_falls_back_alone() {
        let saved = vec![
            InputValue::Int(50),
            InputValue::Bool(true), // a float lives at this index now
        ];
        let built = grown_script()
            .build_with(Some(&saved), &mut Silent)
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(false),
            ],
            "the mistyped cell falls back alone; the value before it survives"
        );
    }
    /// The count-matched case, which used to skip type checks entirely: a
    /// script that swapped an input's type while keeping the same number of
    /// them bound the stale value straight through.
    #[test]
    fn a_type_change_at_an_unchanged_count_no_longer_binds_the_stale_value() {
        let saved = vec![
            InputValue::Int(50),
            InputValue::Bool(true), // the float's index
            InputValue::Bool(true),
        ];
        let built = grown_script()
            .build_with(Some(&saved), &mut Silent)
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(true),
            ],
            "three saved, three declared, and the mistyped one still falls back alone"
        );
    }
    /// An empty value set is a slot that was never given one, not an
    /// instruction to forget the values it holds.
    ///
    /// The two readings are indistinguishable at the call site and differ
    /// only when a native has been tuned: taking "empty" literally rebuilds
    /// it at its declared defaults, which a trader reads as their period
    /// resetting itself. The script arm has always made this distinction;
    /// this asserts the native arm makes the same one.
    #[test]
    fn an_empty_value_set_keeps_a_natives_restored_values() {
        let source = IndicatorSource::Native {
            id: "native.ema".to_owned(),
            values: vec![InputValue::Int(21), InputValue::Source(SourceId::Delta)],
        };

        let built = source
            .build_with(Some(&[]), &mut Silent)
            .expect("the EMA is in the catalog");
        assert_eq!(
            built.descriptor().title,
            "EMA(21, delta)",
            "an empty set falls back to what the source carries"
        );

        // And a real set still wins over the carried one.
        let rebound = source
            .build_with(Some(&[InputValue::Int(5)]), &mut Silent)
            .expect("the EMA is in the catalog");
        assert_eq!(rebound.descriptor().title, "EMA(5)");
    }
}
