//! The Pine scripts a tab arms alarm-only on its panes, in plain words and
//! numbers.
//!
//! The kernel types live in `quantick-script-trigger` and
//! `quantick-strategy`, which the control crates may not reach
//! (`guards/src/graph.rs`); a call arms, lists and disarms through here, as
//! [`crate::strategy_reading`] reads the drawn instances. Each instance rides
//! one pane — the bars it judges are that pane's closes — and is evaluated
//! with the pane's other strategies ([`TabStrategies::evaluate`]).

use quantick_engine::{Bar, Side};
use quantick_script_trigger::{
    Library, Script, ScriptInstance, ScriptPlacement, ScriptState, WARMUP_BARS,
};
use quantick_strategy::{CrossDirection, LineCross, Region};
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;

/// Where a script may ring, in a caller's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPlace<D> {
    /// The rectangle drawing `D` on the pane marks.
    Region(D),
    /// A fixed band between two prices, either order.
    Range { a: Decimal, b: Decimal },
    /// A horizontal line the body must cross, upward when `rising`.
    Line { level: Decimal, rising: bool },
    /// No price condition.
    Anywhere,
}

impl<D: Copy> ScriptPlace<D> {
    fn placement(self) -> ScriptPlacement<D> {
        match self {
            Self::Region(drawing) => ScriptPlacement::Region(drawing),
            Self::Range { a, b } => ScriptPlacement::Range(Region::new(a, b)),
            Self::Line { level, rising } => ScriptPlacement::Line(LineCross {
                level,
                direction: if rising {
                    CrossDirection::Up
                } else {
                    CrossDirection::Down
                },
            }),
            Self::Anywhere => ScriptPlacement::Anywhere,
        }
    }

    fn of(placement: &ScriptPlacement<D>) -> Self {
        match *placement {
            ScriptPlacement::Region(drawing) => Self::Region(drawing),
            ScriptPlacement::Range(band) => Self::Range {
                a: band.low(),
                b: band.high(),
            },
            ScriptPlacement::Line(line) => Self::Line {
                level: line.level,
                rising: line.direction == CrossDirection::Up,
            },
            ScriptPlacement::Anywhere => Self::Anywhere,
        }
    }
}

/// One arm: which script from which library, on which pane, hunting which
/// side, where.
#[derive(Debug, Clone, Copy)]
pub struct ScriptArm<'a, D> {
    pub pane_id: u64,
    /// The library folder the launch named; `None` for the default.
    pub library: Option<&'a str>,
    /// The script's bare file name in the library.
    pub script: &'a str,
    pub side: Side,
    pub place: ScriptPlace<D>,
}

/// One armed script as a caller reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptReading<D> {
    /// Stable for the tab's life: what a disarm names.
    pub id: u64,
    pub pane_id: u64,
    pub script: String,
    pub side: Side,
    pub place: ScriptPlace<D>,
    /// `watching`, or `disarmed` with the reason.
    pub watching: bool,
    /// `line N: <CODE> <message>` for a script that failed; the kernel's
    /// disarm label otherwise.
    pub disarm_reason: Option<String>,
    /// The kernel's badge line: what the last bar did.
    pub status: String,
}

impl<D: Copy> ScriptReading<D> {
    fn of(pane_id: u64, instance: &ScriptInstance<D>) -> Self {
        let disarm_reason = match instance.state() {
            ScriptState::Watching => None,
            ScriptState::Disarmed { reason } => Some(reason),
        };
        Self {
            id: instance.id,
            pane_id,
            script: instance.script.clone(),
            side: instance.side(),
            place: ScriptPlace::of(&instance.placement),
            watching: disarm_reason.is_none(),
            disarm_reason,
            status: instance.armed().status_line(),
        }
    }
}

/// The folder `library` names, or the default one, as a caller shows it.
#[must_use]
pub fn library_folder(library: Option<&str>) -> String {
    Library::resolve(library).dir().display().to_string()
}

/// The script file names in `library` (the default folder for `None`),
/// sorted.
///
/// # Errors
///
/// The folder cannot be read, in words.
pub fn library_scripts(library: Option<&str>) -> Result<Vec<String>, String> {
    Library::resolve(library)
        .names()
        .map_err(|error| error.to_string())
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// Load `request.script` from its library and arm it alarm-only on
    /// `request.pane_id`, warmed on `warmup(depth)`. A script that fails while
    /// warming is armed already disarmed, and its reading says why.
    ///
    /// # Errors
    ///
    /// The sentence the caller reads: an unreadable script, one that does
    /// not compile, or an alert title that names no side.
    pub fn arm_script(
        &mut self,
        request: ScriptArm<'_, D>,
        warmup: impl FnOnce(usize) -> Vec<Bar>,
    ) -> Result<ScriptReading<D>, String> {
        let source = Library::resolve(request.library)
            .read(request.script)
            .map_err(|error| error.to_string())?;
        let script = Script::load(request.script, &source)
            .map_err(|refusal| format!("`{}` cannot be armed: {refusal}", request.script))?;
        let id = self.next_script_id;
        self.next_script_id += 1;
        let instance = ScriptInstance::arm(
            id,
            &script,
            request.side,
            request.place.placement(),
            &warmup(WARMUP_BARS),
        );
        let reading = ScriptReading::of(request.pane_id, &instance);
        self.pane_mut(request.pane_id).scripts.push(instance);
        Ok(reading)
    }

    /// Remove script instance `id`, answering what it was; `None` when no
    /// pane holds it. Alarm-only, so nothing is owed on the way out.
    pub fn disarm_script(&mut self, id: u64) -> Option<ScriptReading<D>> {
        self.panes_mut().find_map(|(pane_id, pane)| {
            let at = pane.scripts.iter().position(|script| script.id == id)?;
            Some(ScriptReading::of(pane_id, &pane.scripts.remove(at)))
        })
    }

    /// Every armed script, panes in id order, each pane's in arming order.
    #[must_use]
    pub fn script_readings(&self) -> Vec<ScriptReading<D>> {
        self.panes()
            .flat_map(|(pane_id, pane)| {
                (pane.scripts.iter()).map(move |script| ScriptReading::of(pane_id, script))
            })
            .collect()
    }
}
