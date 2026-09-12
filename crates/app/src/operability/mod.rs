//! Where every supported UI behaviour says which capability reaches it — or
//! records, in writing, why none does.
//!
//! `CLAUDE.md`'s *Operable without a hand* says no capability ships reachable
//! by mouse alone. The control plane answers half of that: 38 capabilities,
//! generated into `docs/control-plane/capability-inventory.md` and guarded
//! against drift. The other half had no answer at all. Nothing enumerated the
//! behaviours a *trader* can reach, so nothing could say which of them an
//! operator cannot. One gesture — deleting a layout tab — carried a written
//! exclusion beside its capability, in `crate::control::layout` where
//! `layout.tab.delete` is deliberately not registered; every other
//! unreachable behaviour was unreachable silently, and a reviewer asking
//! "what can the mouse do that a script cannot?" had to read the interface.
//!
//! This module is that enumeration. [`registry::UI_BEHAVIOURS`] holds one row
//! per behaviour: what the trader does, where it is reachable, and either the
//! capability identifiers that perform it or a classified exclusion with its
//! reason. `docs/control-plane/ui-behaviour-matrix.md` is that table rendered,
//! by `quantick-app --dump-ui-behaviour-matrix`, the way the capability
//! inventory is rendered from the registry beside it.
//!
//! # Why a table and not a scan
//!
//! A behaviour is an *act* — "sell at market", "collapse the context charts" —
//! and the interface reaches most of them from three places at once: a
//! toolbar button, a menu entry and a hotkey are three doors into one room.
//! No single scan of the source recovers that, and a matrix keyed on widgets
//! instead of acts would report the same behaviour three times and still miss
//! the pane gestures that are drawn rather than registered.
//!
//! So the rows are written, and the *drift guard* is mechanical. Every
//! registry the interface already has — the toolbar's action enum, the strip
//! actions, the dock tabs, the drawing-tool registry, the layout presets, the
//! rail docks, the feed-notice recovery controls, the scripted menus, the
//! keyboard-shortcut constants and the menu bar's own button labels — is
//! walked in `sources`, and every entry it yields must be *claimed* by some
//! row through `UiBehaviour::keys`. A new hotkey, a new drawing tool, a new
//! toolbar action or a new menu entry that no row claims fails `drift` with
//! the entry named, and a row claiming an entry the interface no longer has
//! fails it from the other side.
//!
//! The second half of the guard runs the other way. A row that names a
//! capability the control registry does not register is drift too — that is
//! how a matrix goes stale when a capability is renamed or withdrawn.
//!
//! # The exclusion classes are closed
//!
//! Three, and no fourth without a decision to add one. See
//! [`ExclusionClass`]. An exclusion is a *record*, never a verdict this module
//! is entitled to reach on its own: enabling a capability for an excluded
//! behaviour is a product and authority decision, and a row saying
//! [`ExclusionClass::PendingCapability`] is the request for that decision, not
//! the decision.
//!
//! # Cost
//!
//! Nothing here runs in a frame. The table is `const` data; `drift` and the
//! walks in `sources` are test- and generation-time only, and the renderer
//! is reached from `main`'s dump path before a window exists.

pub(crate) mod matrix;
pub(crate) mod registry;
#[cfg(test)]
pub(crate) mod sources;

#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

/// One behaviour a trader can perform, and what reaches it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UiBehaviour {
    /// Stable identifier, dotted and lowercase like a capability id so the
    /// two read as one namespace. Never the label: a reworded button must not
    /// move an operator's ground.
    pub id: &'static str,
    /// What the trader does, in one line, in their words rather than the
    /// code's.
    pub title: &'static str,
    /// Where it is reachable in the interface — every door, because a
    /// behaviour with three doors and no capability is still one gap, and a
    /// reader needs to know where to look.
    pub reach: &'static str,
    /// The registry entries this row claims. Every entry the interface
    /// registers must be claimed exactly once; see `drift`.
    pub keys: &'static [(Source, &'static str)],
    /// The capabilities that perform it, or the recorded exclusion.
    pub mapping: Mapping,
}

/// A registry the interface already keeps, walked to find what is registered.
///
/// The variant names the registry, never the file: a registry that moves
/// keeps its name here and the rows keep claiming it.
///
/// Closed rather than a trait, and on purpose: a source is not a thing this
/// module dispatches on — `sources::registered` walks each one differently and
/// has to, because a `BTreeMap` of tools and a text scan for `const` lines
/// share no shape. A trait would buy a uniform call and cost the one property
/// that matters here, which is that adding a registry to the interface without
/// telling this enum is a compile error in `as_str` rather than a silently
/// unwalked registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// A `toolbar::ToolbarAction` variant — what the toolbar asks the app to
    /// do.
    ToolbarAction,
    /// A `layout_strip::StripAction` variant.
    StripAction,
    /// A `tabstrip::TabAction` variant.
    TabAction,
    /// A `dock::DockTab` entry.
    DockTab,
    /// A `toolbar::LayerToggle` entry.
    LayerToggle,
    /// A `drawings::DRAWING_TOOLS` entry, by its registered tool id.
    DrawingTool,
    /// A `toolrail::Tool` that is not a drawing tool.
    RailTool,
    /// A `toolrail::ToolboxDock` edge.
    ToolboxDock,
    /// A `canvas_layout::LAYOUT_PRESETS` entry, by preset id.
    LayoutPreset,
    /// A `feed_notice::NoticeAction` variant.
    NoticeAction,
    /// A `KeyboardShortcut` constant, by the constant's own name.
    Hotkey,
    /// A menu-bar entry, by the literal label the source passes to egui.
    MenuEntry,
    /// A `harness::ScriptedMenu` token.
    ScriptedMenu,
    /// No registry stands behind it — a gesture drawn straight onto a pane, or
    /// a field inside a panel. Exempt from the walk in both directions, and
    /// counted apart in the matrix so the reader knows how much of the table
    /// the mechanical guard is holding up.
    Authored,
}

impl Source {
    /// Every source, so the matrix can report per-source counts without a
    /// second list going stale beside this one.
    pub(crate) const ALL: [Self; 14] = [
        Self::ToolbarAction,
        Self::StripAction,
        Self::TabAction,
        Self::DockTab,
        Self::LayerToggle,
        Self::DrawingTool,
        Self::RailTool,
        Self::ToolboxDock,
        Self::LayoutPreset,
        Self::NoticeAction,
        Self::Hotkey,
        Self::MenuEntry,
        Self::ScriptedMenu,
        Self::Authored,
    ];

    /// The name the matrix and every drift message use.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ToolbarAction => "toolbar_action",
            Self::StripAction => "strip_action",
            Self::TabAction => "tab_action",
            Self::DockTab => "dock_tab",
            Self::LayerToggle => "layer_toggle",
            Self::DrawingTool => "drawing_tool",
            Self::RailTool => "rail_tool",
            Self::ToolboxDock => "toolbox_dock",
            Self::LayoutPreset => "layout_preset",
            Self::NoticeAction => "notice_action",
            Self::Hotkey => "hotkey",
            Self::MenuEntry => "menu_entry",
            Self::ScriptedMenu => "scripted_menu",
            Self::Authored => "authored",
        }
    }

    /// Whether `sources::registered` enumerates this source. `Authored` is
    /// the only one it does not, and the only one a row may claim without the
    /// interface registering anything.
    pub(crate) fn is_walked(self) -> bool {
        !matches!(self, Self::Authored)
    }
}

/// What performs a behaviour, or why nothing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mapping {
    /// The capability identifiers that perform it. More than one where the
    /// behaviour is a pair the control plane splits — collapsing and
    /// restoring a pane are two calls and one gesture.
    Capabilities(&'static [&'static str]),
    /// Nothing performs it, on the record.
    Excluded {
        /// Which of the three closed classes the exclusion falls in.
        class: ExclusionClass,
        /// Why, citing the descriptor, the decision or the issue. Prose a
        /// reader can disagree with: an exclusion nobody can argue against is
        /// how a gap becomes permanent.
        reason: &'static str,
    },
}

/// The closed set of reasons a behaviour has no capability.
///
/// Closed on purpose. An open set of excuses is an open set of gaps, and the
/// value of this table is that every row is one of exactly three things: it
/// works, it is refused for a stated reason, or somebody owes a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExclusionClass {
    /// A profile or contract ceiling excludes it on purpose. The reason cites
    /// the ceiling — the threat model's refusals, an effect policy — and the
    /// answer to "why not add it" is that the ceiling says no.
    Authority,
    /// An explicit product decision that the gesture stays the trader's. The
    /// reason cites where that decision is written down.
    UiOnlyByDecision,
    /// Nothing refuses it; nobody has built it. The reason cites the issue
    /// that would. This is the only class that is a debt.
    PendingCapability,
}

impl ExclusionClass {
    /// Every class, for the matrix's summary.
    pub(crate) const ALL: [Self; 3] = [
        Self::Authority,
        Self::UiOnlyByDecision,
        Self::PendingCapability,
    ];

    /// The name the matrix prints and a reviewer greps for.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::UiOnlyByDecision => "ui_only_by_decision",
            Self::PendingCapability => "pending_capability",
        }
    }
}

/// One entry a registry yields: which registry, and the key inside it.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Registered {
    /// The registry it came from.
    pub source: Source,
    /// The key inside that registry.
    pub key: String,
}

#[cfg(test)]
impl Registered {
    /// One registration, for a walk in `sources` or a fixture in a test.
    pub(crate) fn new(source: Source, key: impl Into<String>) -> Self {
        Self {
            source,
            key: key.into(),
        }
    }
}

/// One way the matrix and the interface can disagree.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Drift {
    /// The interface registers something no row claims. The finding this
    /// module exists for: a hotkey, a tool or a menu entry shipped without
    /// anybody deciding whether an operator can reach it.
    Unclaimed {
        /// The registry that yielded it.
        source: Source,
        /// The key nothing claims.
        key: String,
    },
    /// A row claims a registry entry that no longer exists — a tool removed,
    /// a menu entry reworded — so the row is describing an interface that is
    /// gone.
    Orphan {
        /// The row holding the stale claim.
        behaviour: &'static str,
        /// The registry it claims from.
        source: Source,
        /// The key that registry no longer yields.
        key: String,
    },
    /// A row names a capability the control registry does not register.
    UnknownCapability {
        /// The row.
        behaviour: &'static str,
        /// The identifier nothing registers.
        capability: String,
    },
    /// Two rows claim the same registry entry, so "which behaviour is this"
    /// has two answers.
    DuplicateClaim {
        /// The registry.
        source: Source,
        /// The contested key.
        key: String,
        /// The two rows claiming it.
        behaviours: [&'static str; 2],
    },
    /// Two rows share an identifier.
    DuplicateBehaviour {
        /// The identifier used twice.
        id: &'static str,
    },
    /// A row claims a key under `Source::Authored`, which stands for "no
    /// registry" and therefore has nothing to claim.
    AuthoredClaim {
        /// The row.
        behaviour: &'static str,
    },
    /// An entry `NOT_A_BEHAVIOUR` excuses that the interface no longer
    /// registers. Its own variant rather than an `Orphan` with a placeholder
    /// row id, because the message has to send the reader to the excuse list
    /// and not hunting a behaviour that was never in the table.
    StaleExcuse {
        /// The registry.
        source: Source,
        /// The entry the excuse still names.
        key: String,
    },
    /// One entry is both claimed by a row and excused as not a behaviour, so
    /// the two lists contradict each other. Without this the contradiction is
    /// invisible: the entry is skipped for being claimed, and the excuse sits
    /// there forever saying the opposite.
    ClaimedAndExcused {
        /// The registry.
        source: Source,
        /// The entry both lists name.
        key: String,
        /// The row claiming it.
        behaviour: &'static str,
    },
}

#[cfg(test)]
impl Drift {
    /// The line a failing test prints. One line per finding, naming what to
    /// do about it, because the reader is an agent that added a hotkey and
    /// does not yet know this table exists.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unclaimed { source, key } => format!(
                "`{}` registers `{key}`, and no row in `UI_BEHAVIOURS` claims it. Add a row \
                 mapping it to a capability, or to a recorded exclusion, in \
                 crates/app/src/operability/registry.rs",
                source.as_str()
            ),
            Self::Orphan {
                behaviour,
                source,
                key,
            } => format!(
                "behaviour `{behaviour}` claims `{}` entry `{key}`, which the interface no \
                 longer registers. Drop the key, or the row",
                source.as_str()
            ),
            Self::UnknownCapability {
                behaviour,
                capability,
            } => format!(
                "behaviour `{behaviour}` maps to capability `{capability}`, which the control \
                 registry does not register. Fix the identifier, or record an exclusion"
            ),
            Self::DuplicateClaim {
                source,
                key,
                behaviours,
            } => format!(
                "`{}` entry `{key}` is claimed by both `{}` and `{}`",
                source.as_str(),
                behaviours[0],
                behaviours[1]
            ),
            Self::DuplicateBehaviour { id } => {
                format!("two rows share the behaviour id `{id}`")
            }
            Self::StaleExcuse { source, key } => format!(
                "`NOT_A_BEHAVIOUR` excuses `{}` entry `{key}`, which the interface no longer registers. Drop the excuse",
                source.as_str()
            ),
            Self::ClaimedAndExcused {
                source,
                key,
                behaviour,
            } => format!(
                "`{}` entry `{key}` is claimed by behaviour `{behaviour}` and also listed in `NOT_A_BEHAVIOUR`. Drop whichever of the two is wrong",
                source.as_str()
            ),
            Self::AuthoredClaim { behaviour } => format!(
                "behaviour `{behaviour}` claims an `authored` key; `authored` means no registry \
                 stands behind the row, so it carries no key"
            ),
        }
    }
}

/// Compare the matrix against what the interface registers and what the
/// control plane offers.
///
/// A pure function of its four arguments, which is what makes the drift
/// fixture in `matrix` possible: the same comparison runs against a
/// fabricated registration and against the real ones, so the guard is proven
/// to fail rather than merely believed to.
#[cfg(test)]
pub(crate) fn drift(
    behaviours: &[UiBehaviour],
    registered: &[Registered],
    excused: &[(Source, &str, &str)],
    capabilities: &BTreeSet<String>,
) -> Vec<Drift> {
    let mut findings = Vec::new();
    let mut seen_ids: BTreeSet<&'static str> = BTreeSet::new();
    let mut claims: BTreeMap<(Source, &'static str), &'static str> = BTreeMap::new();

    for behaviour in behaviours {
        if !seen_ids.insert(behaviour.id) {
            findings.push(Drift::DuplicateBehaviour { id: behaviour.id });
        }
        for (source, key) in behaviour.keys {
            if *source == Source::Authored {
                findings.push(Drift::AuthoredClaim {
                    behaviour: behaviour.id,
                });
                continue;
            }
            if let Some(previous) = claims.insert((*source, key), behaviour.id) {
                findings.push(Drift::DuplicateClaim {
                    source: *source,
                    key: (*key).to_owned(),
                    behaviours: [previous, behaviour.id],
                });
            }
        }
        if let Mapping::Capabilities(ids) = behaviour.mapping {
            for id in ids {
                if !capabilities.contains(*id) {
                    findings.push(Drift::UnknownCapability {
                        behaviour: behaviour.id,
                        capability: (*id).to_owned(),
                    });
                }
            }
        }
    }

    let registered_keys: BTreeSet<(Source, &str)> = registered
        .iter()
        .map(|entry| (entry.source, entry.key.as_str()))
        .collect();
    let excused_keys: BTreeSet<(Source, &str)> = excused
        .iter()
        .map(|(source, key, _)| (*source, *key))
        .collect();
    for entry in registered {
        let key = (entry.source, entry.key.as_str());
        let claimed = claims.get(&key);
        let excused = excused_keys.contains(&key);
        match (claimed, excused) {
            (Some(behaviour), true) => findings.push(Drift::ClaimedAndExcused {
                source: entry.source,
                key: entry.key.clone(),
                behaviour,
            }),
            (Some(_), false) | (None, true) => {}
            (None, false) => findings.push(Drift::Unclaimed {
                source: entry.source,
                key: entry.key.clone(),
            }),
        }
    }
    for (source, key) in &excused_keys {
        if !registered
            .iter()
            .any(|entry| entry.source == *source && entry.key.as_str() == *key)
        {
            findings.push(Drift::StaleExcuse {
                source: *source,
                key: (*key).to_owned(),
            });
        }
    }
    for ((source, key), behaviour) in &claims {
        if !registered_keys.contains(&(*source, *key)) {
            findings.push(Drift::Orphan {
                behaviour,
                source: *source,
                key: (*key).to_owned(),
            });
        }
    }

    findings
}

/// The capability identifiers the application registers.
///
/// Read out of `docs/control-plane/capability-inventory.md`, which is not a
/// second opinion: that file is generated from the live registry by
/// `control::inventory`, and a test there compares the committed copy against
/// the generator byte for byte on every run. So a stale inventory fails its
/// own guard before it can mislead this one, and the alternative — widening
/// `control`'s private modules so this one can build a second
/// `ObserverContract` — would buy nothing and cost the encapsulation the
/// control plane keeps on purpose.
#[cfg(test)]
pub(crate) fn registered_capability_ids() -> Result<BTreeSet<String>, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("crates/app sits two levels below the workspace root")?
        .join("docs/control-plane/capability-inventory.md");
    let inventory = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let ids: BTreeSet<String> = inventory
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split_once('`'))
        .map(|(id, _)| id.to_owned())
        .collect();
    if ids.is_empty() {
        return Err(format!(
            "{} carries no capability rows; the parse is broken, not the inventory",
            path.display()
        ));
    }
    Ok(ids)
}
