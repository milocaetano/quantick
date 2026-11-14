use super::{Drawing, DrawingEdit, PresetWrite, SavedDefault, quick_range};

/// What the drawing chrome asked the host to do.
///
/// A struct of defaults like [`super::SurfaceResponse`] itself: every field
/// means "this was not asked for" until a surface sets it, so a new request
/// never touches an existing caller. The store commands are named fields
/// rather than an enum for exactly that reason.
#[derive(Debug, Default, PartialEq)]
pub struct DrawingChromeAsk {
    /// Open the alert editor on this drawing.
    pub alert_target: Option<quantick_control_schema::alerts::DrawingSelector>,
    /// Synchronous host continuations, absent on idle frames.
    pub readbacks: Option<Box<Readbacks>>,
    pub clipboard_paste: Option<(Box<Drawing>, u32)>,
    pub inspector_position_dirty: bool,
    pub inline_started: bool,

    /// Replace the temporary secondary-drag ruler with the chosen durable
    /// drawing through its registered annotation action.
    pub place_quick_range: Option<Box<quick_range::PendingRangePlacement>>,
    /// The temporary range stood down because the trader clicked the chart.
    pub dismiss_quick_range: bool,
    /// The inspector edited its copy of the selected drawing. Boxed: it is by
    /// far the largest thing a response can carry — a pencil stroke is 512
    /// anchors — and every other surface would pay for its size in a response
    /// returned by value on every frame.
    pub edited: Option<Box<Drawing>>,
    /// The pointer and the keyboard have let go: record this pre-edit copy as
    /// the one undo entry the whole gesture earned.
    ///
    /// The copy travels with the ask rather than being fetched back off the
    /// surface, so the host applies a response instead of interrogating the
    /// thing that produced it. Boxed for the reason [`Self::edited`] is: a
    /// `Drawing` inline here would be paid for by every surface in the
    /// application, on every frame, in a response returned by value.
    pub commit_edit_gesture: Option<Box<DrawingEdit>>,
    /// Hide or show the selection.
    pub toggle_selected_hidden: bool,
    /// Lock or unlock the selection.
    pub toggle_selected_locked: bool,
    /// Delete the selection, through the confirmation rule the keyboard uses.
    pub request_delete: bool,
    /// Delete the selection even though it is locked — the trader answered
    /// the question.
    pub force_delete: bool,
    /// The trader answered "no" to that question.
    ///
    /// Applied by the host *after* [`Self::request_delete`], not cleared
    /// surface-side during the draw: a frame that carries both — the bar's
    /// confirm glyph and the body's Cancel — must end with the prompt gone,
    /// and a delete request on a locked object raises it again.
    pub cancel_delete: bool,
    /// Copy the selection, offset so the copy is visibly a copy.
    pub duplicate: bool,
    /// Arm this tool again — the selected stroke's own pencil.
    pub draw_again: Option<quantick_drawings::DrawingTool>,
    /// Place a text note in the middle of the pane and open its editor — the
    /// `QUANTICK_TEXT_NOTE` hook, which needs the placement rules the host
    /// owns.
    pub place_text_note: bool,
    /// Record this object's pre-edit copy as one undo entry: the inline
    /// editor closed, on the pane the note actually lives on, which is not
    /// necessarily the one in front. Boxed like the two above.
    pub record_inline_edit: Option<Box<DrawingEdit>>,
    /// The caret moved to a different object, or left the chart. Every pane
    /// has to be told, so exactly one object anywhere stands down; the host
    /// reads the new target back by name.
    pub content_editing_changed: bool,
    /// Select this row, and bring it into view.
    pub manager_select: Option<usize>,
    /// Toggle this row's eye.
    pub manager_toggle_hidden: Option<usize>,
    /// Toggle this row's lock.
    pub manager_toggle_locked: Option<usize>,
    /// Raise this row above the rest.
    pub manager_bring_to_front: Option<usize>,
    /// Delete this row, through the same confirmation the inspector raises.
    pub manager_delete: Option<usize>,
    /// Show every hidden object.
    pub show_all: bool,
    /// Unlock every locked object.
    pub unlock_all: bool,
    /// Delete every object, locked ones included — the count-bearing gate was
    /// answered.
    pub delete_all: bool,
    /// Take back every object an assistant placed on this chart.
    pub sweep_authored: bool,
    /// A named default was remembered, and is owed a line the trader can read.
    pub saved_default: Option<SavedDefault>,
    /// Writes to the preset bank, in the order the frame made them.
    pub presets: Vec<PresetWrite>,
}

impl DrawingChromeAsk {
    /// Fold one piece of chrome's asks into the frame's.
    ///
    /// Flags stay set, so no piece can cancel another's ask by being drawn
    /// after it, and a valued ask keeps the **first** — the same rule
    /// [`super::SurfaceResponse::merge`] follows, for the same reason: two
    /// pieces cannot both be right about one row, and dropping the later ask
    /// deterministically beats letting draw order decide in silence.
    /// Conversion is linear: its sole producer must never contribute twice.
    /// A debug build says so loudly; a release frame keeps the first like
    /// every other valued ask rather than aborting on the UI thread.
    pub fn merge(&mut self, other: Self) {
        if let Some(other) = other.readbacks {
            let own = self.readbacks.get_or_insert_with(Default::default);
            own.scenario = own.scenario.take().or(other.scenario);
            own.quick_range = own.quick_range.take().or(other.quick_range);
        }
        self.clipboard_paste = self.clipboard_paste.take().or(other.clipboard_paste);
        self.inspector_position_dirty |= other.inspector_position_dirty;
        self.inline_started |= other.inline_started;

        debug_assert!(
            self.place_quick_range.is_none() || other.place_quick_range.is_none(),
            "one quick-range conversion producer"
        );
        self.place_quick_range = self.place_quick_range.take().or(other.place_quick_range);
        self.dismiss_quick_range |= other.dismiss_quick_range;
        self.alert_target = self.alert_target.take().or(other.alert_target);
        self.edited = self.edited.take().or(other.edited);
        self.commit_edit_gesture = self
            .commit_edit_gesture
            .take()
            .or(other.commit_edit_gesture);
        self.toggle_selected_hidden |= other.toggle_selected_hidden;
        self.toggle_selected_locked |= other.toggle_selected_locked;
        self.request_delete |= other.request_delete;
        self.force_delete |= other.force_delete;
        self.cancel_delete |= other.cancel_delete;
        self.duplicate |= other.duplicate;
        self.draw_again = self.draw_again.or(other.draw_again);
        {
            self.place_text_note |= other.place_text_note;
        }
        self.record_inline_edit = self.record_inline_edit.take().or(other.record_inline_edit);
        self.content_editing_changed |= other.content_editing_changed;
        self.manager_select = self.manager_select.or(other.manager_select);
        self.manager_toggle_hidden = self.manager_toggle_hidden.or(other.manager_toggle_hidden);
        self.manager_toggle_locked = self.manager_toggle_locked.or(other.manager_toggle_locked);
        self.manager_bring_to_front = self.manager_bring_to_front.or(other.manager_bring_to_front);
        self.manager_delete = self.manager_delete.or(other.manager_delete);
        self.show_all |= other.show_all;
        self.unlock_all |= other.unlock_all;
        self.delete_all |= other.delete_all;
        self.sweep_authored |= other.sweep_authored;
        self.saved_default = self.saved_default.or(other.saved_default);
        self.presets.extend(other.presets);
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Readbacks {
    pub scenario: Option<super::scenario::Effect>,
    pub quick_range:
        Option<quantick_chart_interaction::quick_range::conversion_plan::ConversionReadback>,
}
impl DrawingChromeAsk {
    pub fn scenario(effect: Option<super::scenario::Effect>) -> Self {
        Self {
            readbacks: effect.map(|effect| {
                Box::new(Readbacks {
                    scenario: Some(effect),
                    ..Default::default()
                })
            }),
            ..Default::default()
        }
    }
    pub fn take_scenario(&mut self) -> Option<super::scenario::Effect> {
        self.readbacks.as_mut()?.scenario.take()
    }
    pub fn take_quick_range_readback(
        &mut self,
    ) -> Option<quantick_chart_interaction::quick_range::conversion_plan::ConversionReadback> {
        self.readbacks.as_mut()?.quick_range.take()
    }
    pub fn set_quick_range_readback(
        &mut self,
        value: quantick_chart_interaction::quick_range::conversion_plan::ConversionReadback,
    ) {
        self.readbacks
            .get_or_insert_with(Default::default)
            .quick_range = Some(value);
    }
}
