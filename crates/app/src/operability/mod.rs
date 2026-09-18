//! The interface's half of the operability contract.
//!
//! The table of behaviours, the matrix renderer and the drift comparison are
//! `quantick-operability`, a headless crate: they are data and a pure
//! function over it. What only this crate can supply is the walk of its own
//! registries — the toolbar actions, the drawing tools, the hotkeys, the menu
//! entries — and that is [`sources`], test-only because reading
//! `crates/app/src` at runtime is meaningless in a shipped binary. The tests
//! below run the shared comparison against that walk, so the guard proves the
//! table against the interface it describes rather than against itself.

#[cfg(test)]
pub(crate) mod sources;

pub(crate) use quantick_operability::matrix;
#[cfg(test)]
pub(crate) use quantick_operability::{Registered, Source};

#[cfg(test)]
mod tests {
    use quantick_operability::registry::{NOT_A_BEHAVIOUR, UI_BEHAVIOURS};
    use quantick_operability::{
        Drift, ExclusionClass, Mapping, Registered, Source, UiBehaviour, drift,
        registered_capability_ids,
    };

    use super::sources;

    /// The authoritative check on the table: every behaviour the interface
    /// registers has a row, every row claims something that exists, and every
    /// capability a row names is registered.
    ///
    /// This is the test an agent adding a hotkey or a drawing tool will see
    /// fail, so it prints every finding rather than the first.
    #[test]
    fn the_matrix_covers_every_registered_behaviour() {
        let capabilities = registered_capability_ids().expect("the control registry builds");
        let findings = drift(
            UI_BEHAVIOURS,
            &sources::registered(),
            NOT_A_BEHAVIOUR,
            &capabilities,
        );
        assert!(
            findings.is_empty(),
            "the UI behaviour matrix and the interface disagree:\n{}",
            findings
                .iter()
                .map(|finding| format!("  - {}", finding.message()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// A row that claims nothing and declares nothing is drift, not a gesture.
    ///
    /// The two used to be the same thing: a row with no keys was counted among
    /// the behaviours no registry stands behind, and the generated document
    /// said so to its reader. A forgotten key looks exactly like that from the
    /// outside, so the document was making a claim about the interface on the
    /// evidence of an empty list.
    #[test]
    fn a_row_that_claims_and_declares_nothing_is_drift() {
        let capabilities = registered_capability_ids().expect("the inventory parses");
        let rows = [UiBehaviour {
            id: "fixture.forgotten",
            title: "A row whose author forgot the key",
            reach: "somewhere",
            keys: &[],
            mapping: Mapping::Excluded {
                class: ExclusionClass::PendingCapability,
                reason: "a fixture, long enough for the legibility check to accept it",
            },
        }];

        let findings = drift(&rows, &[], &[], &capabilities);
        assert_eq!(
            findings,
            vec![Drift::Unclassified {
                behaviour: "fixture.forgotten",
            }]
        );
    }

    /// And a declaration is accepted rather than treated as a claim on some
    /// registry: an `authored` key is a reason, so no walk can orphan it.
    #[test]
    fn an_authored_declaration_is_not_a_claim() {
        let capabilities = registered_capability_ids().expect("the inventory parses");
        let rows = [UiBehaviour {
            id: "fixture.gesture",
            title: "A drag nothing registers",
            reach: "the canvas",
            keys: &[(
                Source::Authored,
                "a pointer drag the canvas handles directly",
            )],
            mapping: Mapping::Excluded {
                class: ExclusionClass::PendingCapability,
                reason: "a fixture, long enough for the legibility check to accept it",
            },
        }];

        assert!(drift(&rows, &[], &[], &capabilities).is_empty());
    }

    /// Every source the enum calls walked is actually walked.
    ///
    /// `Source` is closed on purpose — a registry the enum does not know about
    /// is one nothing walks — but closing it puts the variant, its name and
    /// its walk in three places, and the failure that costs is silent: a
    /// variant added and never added to `registered`, reported in the
    /// document's appendix as a source holding rows while nothing checks it.
    /// This is the assertion that makes that failure loud.
    #[test]
    fn every_walked_source_yields_at_least_one_registration() {
        let registered = sources::registered();
        for source in Source::ALL {
            if !source.is_walked() {
                continue;
            }
            assert!(
                registered.iter().any(|entry| entry.source == source),
                "`Source::{}` says it is walked and `sources::registered` yields nothing for it",
                source.as_str()
            );
        }
    }

    /// The drift fixture: a behaviour registered with no row is named, not
    /// tolerated.
    ///
    /// It registers a drawing tool that does not exist rather than adding a
    /// real one, so the guard is proven to fail without the proof itself
    /// shipping a tool nobody asked for. The same comparison runs here and in
    /// the test above; only the input differs.
    #[test]
    fn a_registered_behaviour_with_no_row_is_drift() {
        let capabilities = registered_capability_ids().expect("the control registry builds");
        let mut registered = sources::registered();
        registered.push(Registered::new(Source::DrawingTool, "fixture-tool"));

        let findings = drift(UI_BEHAVIOURS, &registered, NOT_A_BEHAVIOUR, &capabilities);
        assert_eq!(
            findings,
            vec![Drift::Unclaimed {
                source: Source::DrawingTool,
                key: "fixture-tool".to_owned(),
            }],
            "a tool registered with no matrix row has to be the only finding, and has to be one"
        );
        assert!(
            findings[0].message().contains("fixture-tool"),
            "the message names the entry: {}",
            findings[0].message()
        );
    }

    /// The other half of the fixture: a row naming a capability the control
    /// registry does not have is drift too. That is how the matrix goes stale
    /// when a capability is renamed or withdrawn rather than when a button is
    /// added.
    #[test]
    fn a_row_naming_an_unregistered_capability_is_drift() {
        let mut capabilities = registered_capability_ids().expect("the control registry builds");
        let withdrawn = "attention.mark.create";
        assert!(
            capabilities.remove(withdrawn),
            "the fixture withdraws a capability that exists today"
        );

        let findings = drift(
            UI_BEHAVIOURS,
            &sources::registered(),
            NOT_A_BEHAVIOUR,
            &capabilities,
        );
        assert_eq!(
            findings,
            vec![Drift::UnknownCapability {
                behaviour: "attention.mark.create",
                capability: withdrawn.to_owned(),
            }],
            "withdrawing a capability has to leave exactly the rows that named it failing"
        );
    }

    /// A row claiming a registry entry that no longer exists is drift from the
    /// other direction — the matrix describing an interface that is gone.
    #[test]
    fn a_row_claiming_a_departed_entry_is_drift() {
        let capabilities = registered_capability_ids().expect("the control registry builds");
        let registered: Vec<Registered> = sources::registered()
            .into_iter()
            .filter(|entry| !(entry.source == Source::DrawingTool && entry.key == "brush"))
            .collect();

        let findings = drift(UI_BEHAVIOURS, &registered, NOT_A_BEHAVIOUR, &capabilities);
        assert_eq!(
            findings,
            vec![Drift::Orphan {
                behaviour: "tool.brush",
                source: Source::DrawingTool,
                key: "brush".to_owned(),
            }],
            "removing a tool has to leave its row failing"
        );
    }

    /// An entry that is both claimed by a row and excused as not a behaviour
    /// is a contradiction between the two lists, and it has to be named.
    ///
    /// Without this it is the one drift the guard cannot see: the entry is
    /// skipped for being claimed, and the excuse sits beside it forever saying
    /// the opposite.
    #[test]
    fn an_entry_both_claimed_and_excused_is_drift() {
        let capabilities = registered_capability_ids().expect("the inventory parses");
        let mut excused = NOT_A_BEHAVIOUR.to_vec();
        excused.push((
            Source::Hotkey,
            "MARK_SHORTCUT",
            "a fixture contradiction: a real row already claims this hotkey",
        ));

        let findings = drift(
            UI_BEHAVIOURS,
            &sources::registered(),
            &excused,
            &capabilities,
        );
        assert_eq!(
            findings,
            vec![Drift::ClaimedAndExcused {
                source: Source::Hotkey,
                key: "MARK_SHORTCUT".to_owned(),
                behaviour: "attention.mark.create",
            }],
            "excusing an entry a row already claims has to be the only finding, and has to be one"
        );
    }

    /// An excuse for something the interface no longer registers is drift, and
    /// it says so in its own words: the reader is sent to `NOT_A_BEHAVIOUR`,
    /// not hunting a row id that was never in the table.
    #[test]
    fn an_excuse_for_a_departed_entry_is_drift() {
        let capabilities = registered_capability_ids().expect("the inventory parses");
        let mut excused = NOT_A_BEHAVIOUR.to_vec();
        excused.push((
            Source::MenuEntry,
            "A Menu Nothing Draws",
            "a fixture: an excuse left behind by a menu entry that is gone",
        ));

        let findings = drift(
            UI_BEHAVIOURS,
            &sources::registered(),
            &excused,
            &capabilities,
        );
        assert_eq!(
            findings,
            vec![Drift::StaleExcuse {
                source: Source::MenuEntry,
                key: "A Menu Nothing Draws".to_owned(),
            }]
        );
        assert!(
            findings[0].message().contains("NOT_A_BEHAVIOUR"),
            "the message sends the reader to the excuse list: {}",
            findings[0].message()
        );
    }
}
