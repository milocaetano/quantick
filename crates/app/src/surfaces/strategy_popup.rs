//! The arming dialog, as a [`Surface`].
//!
//! "Arm strategy" is the form a trader fills in over a drawn region: side,
//! quantity, the force band, the projected brackets, the alarm — and then
//! **Arm**. It was the largest single thing left in the trunk that was not
//! `draw_frame` itself, and it is here for the reason the port exists: none
//! of what it does needs `QuantickApp`. It needs a preset bank nothing else
//! reads, a form it owns, and three answers from the application it can ask
//! for by name.
//!
//! # What it asks the host for, and why each one
//!
//! **Arming** reaches into the pane, the simulator and the alarm scheduler,
//! and can be refused for a reason the form has to show. So the dialog asks
//! through [`SurfaceResponse::arm_strategy`] and stays open until
//! [`StrategyPopupSurface::settle_arm`] answers — closing on success, and
//! writing the refusal into the footer otherwise. A dialog that closed
//! optimistically and reopened on failure would lose the form.
//!
//! **Auditioning a sound** goes through the host's one speaker, shared with
//! every armed instance, so a test press reports a sound that cannot be heard
//! exactly as a missed signal would. The cue is handed over; the host plays
//! it.
//!
//! **Whether a share of the bar means anything** is a property of the bar
//! rule the pane is running, which the dialog reads from
//! [`SurfaceEnv::counted_bar_sides`] rather than by reaching for the tab.
//!
//! # The alarm section
//!
//! `draw_alarm_controls` is its own function because it is the one part of
//! the form that is about hearing rather than trading, and because the fields
//! under the checkbox are only meaningful while it is ticked — a shape the
//! rest of the dialog does not have. It moved with the dialog it is a section
//! of; it was never separable from it.

use eframe::egui;

use super::{ArmRequest, Surface, SurfaceEnv, SurfaceResponse};
use crate::audio::{AlertSound, Cue, SoundCategory};
use crate::drawings;
use crate::pane;
use crate::strategy_presets::{self as presets, StrategyBank};
use crate::theme;

/// How much of the viewport's height the arming dialog's scrolling body may
/// take. The rest pays for the window's own chrome, its Arm/Cancel footer and
/// the margin that keeps a centred dialog off the chart's edges.
///
/// A fraction rather than a fixed height because the form's length is not
/// fixed either: unfolding the alarm section adds six rows, and on a laptop
/// that was enough to push **Arm** past the bottom of a window the trader
/// cannot resize. Sized so the whole dialog — body, footer and chrome — sits
/// comfortably inside the shortest viewport the app is used on.
const ARM_DIALOG_BODY_SCREEN_FRACTION: f32 = 0.45;

/// Floor under that fraction. On a viewport too short for even this the form
/// scrolls within it rather than collapsing to nothing — a dialog whose Arm
/// button cannot be reached is worse than one that scrolls.
const ARM_DIALOG_MIN_BODY_PT: f32 = 200.0;

/// The arming dialog's state: which drawing on which pane of which tab, and
/// the form — the stored-preset shape edited in place, so "form", "bank row"
/// and "what a future NL layer emits" stay one structure.
struct StrategyPopup {
    /// The **stable id** of the tab the dialog was opened over. Drawing ids
    /// are per-pane counters, so the same id on another tab is an unrelated
    /// object — switching tabs closes the dialog rather than arm it.
    ///
    /// The id and not the index, which is what this was before it moved here.
    /// `close_tab` clamps `active_tab` rather than shifting it, so closing a
    /// tab to the left of the active one leaves the index pointing at a
    /// *different market* while comparing equal — and Arm would then act on
    /// that market. Every other cross-tab reference in the application
    /// (`indicator_settings_target`, `slot_kinds`, `operator_slots`,
    /// `script_files`) keys on the id for exactly this reason.
    tab: u64,
    side: pane::PaneSide,
    drawing: drawings::DrawingId,
    form: presets::StoredPreset,
    /// The bank preset the form was seeded from, shown on the badge.
    preset_choice: Option<String>,
    save_name: String,
    error: Option<String>,
}

/// The "Arm strategy" dialog and the preset bank it reads from.
pub(crate) struct StrategyPopupSurface {
    popup: Option<StrategyPopup>,
    /// The saved forms offered in the dialog's preset list. Nothing else in
    /// the application reads them, so they live with the one window that
    /// does.
    bank: StrategyBank,
    /// One-shot: the next draw of the dialog drops the sound list open. Set
    /// by the `QUANTICK_STRATEGY_DEMO=sounds` hook, consumed by
    /// `draw_alarm_controls` on the first frame it draws the box.
    pending_sound_picker: bool,
}

impl Default for StrategyPopupSurface {
    /// Loads the bank, like the footprint window loads its presets: a list
    /// that stays empty until something remembers to call a second
    /// initialiser is a list that ships empty the one time nobody does.
    /// `StrategyBank::default_path` already redirects under `cfg!(test)`, so
    /// this reads a test file rather than the trader's saved strategies.
    fn default() -> Self {
        Self {
            popup: None,
            bank: StrategyBank::load_from(StrategyBank::default_path()),
            pending_sound_picker: false,
        }
    }
}

impl StrategyPopupSurface {
    /// Whether the dialog is on screen — read by the host each frame, because
    /// the per-side bar-rule answer the dialog reads is built for it alone.
    pub fn is_open(&self) -> bool {
        self.popup.is_some()
    }

    /// Open the dialog over `drawing`, starting from `form`.
    ///
    /// One door for every way in: a pane's right-click request, the capture
    /// hook, and any named call a future operator layer makes.
    pub fn open(
        &mut self,
        tab: u64,
        side: pane::PaneSide,
        drawing: drawings::DrawingId,
        form: presets::StoredPreset,
    ) {
        self.popup = Some(StrategyPopup {
            tab,
            side,
            drawing,
            form,
            preset_choice: None,
            save_name: String::new(),
            error: None,
        });
    }

    /// Drop the sound list open on the next frame the dialog draws it.
    pub fn stage_sound_picker(&mut self) {
        self.pending_sound_picker = true;
    }

    /// The host's answer to [`SurfaceResponse::arm_strategy`]: the instance
    /// armed, or the reason it could not be.
    ///
    /// A refusal keeps the form exactly as the trader left it — they are one
    /// corrected field away from an arming that works, and a dialog that
    /// vanished and reopened empty would make the refusal read as a crash.
    pub fn settle_arm(&mut self, outcome: Result<(), String>) {
        match outcome {
            Ok(()) => self.popup = None,
            Err(error) => {
                if let Some(popup) = self.popup.as_mut() {
                    popup.error = Some(error);
                }
            }
        }
    }

    /// The refusal on screen, if any.
    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.popup.as_ref().and_then(|popup| popup.error.as_deref())
    }

    /// The form as it stands, for the tests that fill it in.
    #[cfg(test)]
    pub fn form_mut(&mut self) -> Option<&mut presets::StoredPreset> {
        self.popup.as_mut().map(|popup| &mut popup.form)
    }

    /// The alarm section of the arming dialog.
    ///
    /// Its own function because it is the one part of the form that is
    /// about hearing rather than trading, and because the fields under the
    /// checkbox are only meaningful while it is ticked — a shape the rest
    /// of the dialog does not have.
    fn draw_alarm_controls(
        &mut self,
        ui: &mut egui::Ui,
        side: pane::PaneSide,
        form: &mut presets::StoredPreset,
        env: &SurfaceEnv<'_>,
        test_alert: &mut Option<Cue>,
    ) {
        // Taken before the early return below, so it is a one-shot whatever
        // the form looks like. Left until the combo row, a staged flag on a
        // form whose alarm box is clear would survive to a later arming and
        // drop the sound list open under the trader's cursor.
        let drop_sound_list_open = std::mem::take(&mut self.pending_sound_picker);

        ui.checkbox(&mut form.alarm, "alarm on signal bar")
            .on_hover_text(
                "play a sound the moment this strategy's signal happens — the trigger \
                 fires on your side, inside the region. It is the *signal* that alarms, \
                 not the order: with the share option below, you hear it before the bar \
                 closes and before any order could be placed, which is the time you need \
                 to act on another platform.",
            );
        if !form.alarm {
            // The alarm-only mode goes with the alarm: an instance that
            // neither trades nor alarms does nothing, and the form must not
            // be able to describe one.
            form.alarm_only = false;
            return;
        }

        // The bar rule the chart is actually running decides whether a
        // share of the bar means anything. An adaptive rule closes on a
        // condition, not on a count, so there is no fraction of it to wait
        // for — and saying so here is cheaper than a trader wondering for a
        // session why the alarm only ever speaks at the close.
        // The pane the dialog is arming on, not the focused one: with a
        // split open, a strategy going onto the time pane must be judged by
        // the time pane's bar rule. Reading the focused pane would disable
        // the share gate because the *other* pane runs an adaptive rule.
        let shares_available = env.counted_bar_sides.contains(&side);

        ui.horizontal(|ui| {
            ui.label("when:");
            let on_close = form.alarm_when != "share";
            if ui
                .selectable_label(on_close, "bar closes")
                .on_hover_text("the same instant the strategy itself judges")
                .clicked()
            {
                form.alarm_when = "on_close".to_owned();
            }
            let share_button = ui.add_enabled(
                shares_available,
                egui::SelectableLabel::new(!on_close, "part-way through the bar"),
            );
            if share_button.clicked() {
                form.alarm_when = "share".to_owned();
            }
            if !shares_available {
                share_button.on_hover_text(
                    "this bar rule closes on a condition rather than on a count, so there \
                     is no share of it to wait for — the alarm speaks at the close",
                );
            }
        });
        if form.alarm_when == "share" {
            ui.horizontal(|ui| {
                ui.label("from");
                ui.add(
                    egui::DragValue::new(&mut form.alarm_share_percent)
                        .range(presets::MIN_ALARM_SHARE_PERCENT..=presets::MAX_ALARM_SHARE_PERCENT),
                );
                ui.label("% of the bar onward").on_hover_text(
                    "on a 2000-tick chart at 70%, the alarm starts judging past tick \
                     1400. The bar is still moving, so the signal is marked \"preview\" \
                     — and if it stops qualifying before the close, the chart says so.",
                );
            });
        }

        ui.horizontal(|ui| {
            ui.label("repeat:");
            let once = form.alarm_repeat != "cooldown";
            if ui
                .selectable_label(once, "once per bar")
                .on_hover_text("one sound per bar, however many prints agree")
                .clicked()
            {
                form.alarm_repeat = "once_per_bar".to_owned();
            }
            if ui.selectable_label(!once, "every").clicked() {
                form.alarm_repeat = "cooldown".to_owned();
            }
            if form.alarm_repeat == "cooldown" {
                ui.add(
                    egui::DragValue::new(&mut form.alarm_cooldown_secs)
                        .range(presets::MIN_ALARM_COOLDOWN_SECS..=presets::MAX_ALARM_COOLDOWN_SECS),
                );
                ui.label("s").on_hover_text(
                    "counted across bars, not reset by one closing — the rule for a \
                     trader who wants a reminder rather than one notice",
                );
            }
        });

        let current = AlertSound::from_token(&form.alarm_sound).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("sound");
            const SOUND_PICKER_ID: &str = "strategy_alarm_sound";
            /// The shortest the sound list is allowed to be, in points:
            /// a heading and two names, enough to see that it scrolls.
            const SOUND_PICKER_MIN_HEIGHT: f32 = 72.0;
            if drop_sound_list_open {
                // The capture hook's one click: open the list the way the
                // button's own click does. The popup id is the widget id
                // plus "popup", and the widget id is the salt *as an `Id`*
                // under this `Ui` — how `ComboBox::from_id_salt` derives
                // it. The one private detail this hook depends on; a
                // capture that opens nothing is the symptom if egui moves
                // it.
                let button_id = ui.make_persistent_id(egui::Id::new(SOUND_PICKER_ID));
                ui.memory_mut(|memory| memory.open_popup(button_id.with("popup")));
            }
            // The list scrolls inside the room under the button. egui flips
            // a combo above its button only from the size it remembered
            // last frame, and a thirty-two-row list opened from the last
            // rows of a dialog docked at the window's foot otherwise runs
            // off the screen; a shorter list that scrolls is the honest
            // answer, and the headings keep the scroll short.
            let room_below = ui.ctx().screen_rect().bottom()
                - ui.next_widget_position().y
                - ui.spacing().interact_size.y
                - ui.spacing().menu_margin.sum().y;
            // `min` then `max`, not `clamp`: `clamp` asserts its bounds are
            // ordered, and a style whose combo height is under the floor
            // must shorten the list, not crash the dialog.
            let list_height = room_below
                .min(ui.spacing().combo_height)
                .max(SOUND_PICKER_MIN_HEIGHT);
            egui::ComboBox::from_id_salt(SOUND_PICKER_ID)
                .selected_text(current.label())
                .height(list_height)
                .show_ui(ui, |ui| {
                    // Grouped under the catalogue's headings: five system
                    // beeps, then the clips that behave like alarms, then
                    // the ones that behave like a room. A flat list of
                    // thirty-two names would make the trader read every
                    // row to find the phone.
                    for category in SoundCategory::ALL {
                        ui.label(egui::RichText::new(category.label()).small().weak());
                        for sound in AlertSound::in_category(category) {
                            if ui
                                .selectable_label(sound == current, sound.label())
                                .clicked()
                            {
                                form.alarm_sound = sound.token().to_owned();
                            }
                        }
                    }
                });
            if ui
                .button("Test")
                .on_hover_text(
                    "play it now, cut where the row below says, so the sound is chosen \
                     with the ears",
                )
                .clicked()
            {
                // Same door as a real alarm, and the same cue — length
                // included — so an audition that cannot be heard reports
                // itself exactly as a missed signal would, and one that can
                // is what the signal will sound like.
                // The speaker is the host's — one sink, shared with every
                // armed instance — so the audition is asked for rather than
                // played here. Same door, same cue, and the same report of a
                // sound that could not be heard.
                *test_alert = Some(Cue::new(current, form.alarm_play_secs));
            }
        });

        ui.horizontal(|ui| {
            let mut cut = form.alarm_play_secs.is_some();
            if ui
                .checkbox(&mut cut, "stop after")
                .on_hover_text(
                    "cut the sound here rather than letting it run to its end — a nature \
                     clip runs for minutes, and an alarm that outstays its news is one the \
                     trader learns to talk over. Off, the sound plays whole.",
                )
                .changed()
            {
                form.alarm_play_secs = cut.then_some(presets::DEFAULT_ALARM_PLAY_SECS);
            }
            if let Some(secs) = form.alarm_play_secs.as_mut() {
                ui.add(
                    egui::DragValue::new(secs)
                        .range(presets::MIN_ALARM_PLAY_SECS..=presets::MAX_ALARM_PLAY_SECS),
                );
                ui.label("s");
                if !current.can_be_cut() {
                    // Honest rather than silent: the cut is stored and
                    // applies the moment a clip is picked, but this sound
                    // is one beep the operating system plays whole.
                    ui.weak("(a system sound is one beep — the cut applies to the clips)");
                }
            }
        });

        ui.checkbox(&mut form.alarm_only, "alarm only — never place an order")
            .on_hover_text(
                "the instance watches, judges and alarms, and places nothing. For a \
                 trader who executes elsewhere: a simulated position they never meant \
                 to take would occupy the account and silence the next signal.",
            );
        if let Some(reason) = env.alert_failure {
            ui.colored_label(theme::SELL, format!("no sound was played: {reason}"));
        }
    }
}

impl Surface for StrategyPopupSurface {
    fn id(&self) -> &'static str {
        "strategy-popup"
    }

    /// No hook of its own, on purpose. The dialog arms a **drawn region**, so
    /// staging it means placing that region first — a form opened over a
    /// drawing that does not exist photographs a dialog nobody could press
    /// Arm on. `QUANTICK_STRATEGY_DEMO` therefore stages both, from the host
    /// that owns the drawing, and reaches this surface through [`Self::open`]
    /// and [`Self::stage_sound_picker`]: the same door a right-click uses.
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {}

    /// The arming dialog. Drains the panes' menu requests first, so the
    /// click that chose "Add strategy…" opens the form on this same frame.
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let Some(mut popup) = self.popup.take() else {
            return SurfaceResponse::default();
        };
        // The dialog speaks for one drawing on one tab. Switching tabs
        // closes it: drawing ids are per-pane counters, and the same id
        // over there names an unrelated object.
        if popup.tab != env.active_tab {
            return SurfaceResponse::default();
        }
        let mut open = true;
        let mut done = false;
        let mut arm = None;
        let mut test_alert = None;
        // The form grew past a 900 pt window once the alarm section unfolds,
        // and an anchored, non-resizable window simply clipped the rows past
        // the edge — including **Arm**, which makes the dialog unusable at
        // that height rather than merely cramped. The body scrolls instead,
        // and its ceiling is read from the viewport rather than fixed, so the
        // same form fits a laptop and still uses a tall monitor.
        let max_body = (ctx.screen_rect().height() * ARM_DIALOG_BODY_SCREEN_FRACTION)
            .max(ARM_DIALOG_MIN_BODY_PT);
        egui::Window::new("Arm strategy")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(max_body)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("preset");
                            let current = popup.preset_choice.as_deref().unwrap_or("custom");
                            egui::ComboBox::from_id_salt("strategy_preset_pick")
                                .selected_text(current.to_owned())
                                .show_ui(ui, |ui| {
                                    let names: Vec<String> =
                                        self.bank.names().map(str::to_owned).collect();
                                    for name in names {
                                        let picked =
                                            popup.preset_choice.as_deref() == Some(name.as_str());
                                        if ui.selectable_label(picked, &name).clicked()
                                            && let Some(stored) = self.bank.get(&name)
                                        {
                                            popup.form = stored.clone();
                                            popup.preset_choice = Some(name.clone());
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("side");
                            let buy = popup.form.side == "buy";
                            if ui.selectable_label(buy, "BUY").clicked() {
                                popup.form.side = "buy".to_owned();
                            }
                            if ui.selectable_label(!buy, "SELL").clicked() {
                                popup.form.side = "sell".to_owned();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("quantity");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.quantity)
                                    .desired_width(60.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("force band: body between");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.min_factor)
                                    .desired_width(40.0),
                            );
                            ui.label("× and");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.max_factor)
                                    .desired_width(40.0),
                            );
                            ui.label("× the average of");
                            ui.add(
                                egui::DragValue::new(&mut popup.form.window)
                                    .range(1..=crate::strategy_presets::MAX_FORCE_WINDOW),
                            );
                            ui.label("bodies");
                        });
                        ui.horizontal(|ui| {
                            ui.label("and candle ≥");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.min_size)
                                    .desired_width(50.0),
                            );
                            ui.label("pts (0 = off)").on_hover_text(
                                "the elephant floor, measured across the whole candle (high − low, \
                                 wicks included): the relative band alone marks dozens of small bars \
                                 as force on activity-cut bars; an elephant has a size",
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("projection: TP");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.tp_mult)
                                    .desired_width(40.0),
                            );
                            ui.label("× range ahead, SL");
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.form.sl_mult)
                                    .desired_width(40.0),
                            );
                            ui.label("× range behind (0 = no leg)");
                        });
                        let mut auto = popup.form.rearm == "auto";
                        if ui
                            .checkbox(&mut auto, "re-arm automatically after the operation closes")
                            .on_hover_text("off = one shot per arming, the over-fire guard")
                            .changed()
                        {
                            popup.form.rearm = if auto { "auto" } else { "one_shot" }.to_owned();
                        }
                        let mut retest = popup.form.on_break == "retest_limit";
                        if ui
                            .checkbox(&mut retest, "on a cut: rest a limit at the region edge")
                            .on_hover_text(
                                "a trigger bar whose body cuts the region in the trade's direction \
                         — it opened on the region's side of that edge and closed beyond it, \
                         wicks ignored — rests a limit at the edge it cut, the retest entry, \
                         bracketed off the bar. The order removes itself if the bar's \
                         projected target trades first; with the TP multiplier at 0 there is \
                         no such level, so it rests until it fills or you disarm it — the \
                         badge says which. A bar that closed past an edge its body never \
                         crossed, one that closed away on the far side, and a cut whose legs \
                         would not clear the edge all rest nothing. Off = a cut holds fire, \
                         as before.",
                            )
                            .changed()
                        {
                            popup.form.on_break =
                                if retest { "retest_limit" } else { "ignore" }.to_owned();
                        }
                        ui.separator();
                        self.draw_alarm_controls(
                            ui,
                            popup.side,
                            &mut popup.form,
                            env,
                            &mut test_alert,
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut popup.save_name)
                                    .hint_text("preset name")
                                    .desired_width(140.0),
                            );
                            let name = popup.save_name.trim().to_owned();
                            if ui
                                .add_enabled(!name.is_empty(), egui::Button::new("Save preset"))
                                .clicked()
                            {
                                self.bank.save(&name, popup.form.clone());
                                popup.preset_choice = Some(name);
                            }
                            if let Some(chosen) = popup.preset_choice.clone()
                                && ui
                                    .button("Delete preset")
                                    .on_hover_text(
                                        "remove it from the bank; the form keeps its values",
                                    )
                                    .clicked()
                            {
                                self.bank.remove(&chosen);
                                popup.preset_choice = None;
                            }
                        });
                    });
                // Outside the scroll: **Arm** is the one control the dialog
                // exists for, and a form that grows must never be able to
                // push it off the bottom. Body scrolls, footer stays. The
                // error goes with it — a refusal the trader has to scroll to
                // find reads as a dialog that did nothing.
                if let Some(error) = &popup.error {
                    ui.colored_label(theme::SELL, error);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Arm").clicked() {
                        let label = popup
                            .preset_choice
                            .clone()
                            .or_else(|| {
                                let name = popup.save_name.trim();
                                (!name.is_empty()).then(|| name.to_owned())
                            })
                            .unwrap_or_else(|| "custom".to_owned());
                        // Asked for, not performed: arming reaches into the
                        // pane, the simulator and the alarm scheduler, none of
                        // which a surface may touch. The dialog therefore
                        // stays open until the host answers through
                        // [`Self::settle_arm`] — which closes it on success
                        // and writes the refusal into the footer otherwise.
                        arm = Some(ArmRequest {
                            side: popup.side,
                            drawing: popup.drawing,
                            form: Box::new(popup.form.clone()),
                            label,
                        });
                    }
                    if ui.button("Cancel").clicked() {
                        done = true;
                    }
                });
            });
        if !done && open {
            self.popup = Some(popup);
        }
        SurfaceResponse {
            arm_strategy: arm,
            test_alert,
            ..SurfaceResponse::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn env(active_tab: u64) -> SurfaceEnv<'static> {
        SurfaceEnv {
            active_tab,
            ..SurfaceEnv::quiet(Instant::now())
        }
    }

    fn form() -> presets::StoredPreset {
        presets::StoredPreset::starting_point(quantick_engine::Side::Buy)
    }

    /// A dialog nobody opened asks for nothing, and costs one `Option` test.
    #[test]
    fn a_closed_dialog_asks_for_nothing() {
        let ctx = egui::Context::default();
        let mut surface = StrategyPopupSurface::default();
        assert!(!surface.is_open());
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(0));
        });
        assert_eq!(response, SurfaceResponse::default());
    }

    /// Drawing ids are per-pane counters, so the same id on another tab names
    /// an unrelated object. Switching tabs therefore closes the dialog rather
    /// than arming the wrong drawing.
    #[test]
    fn switching_tabs_closes_the_dialog() {
        let ctx = egui::Context::default();
        let mut surface = StrategyPopupSurface::default();
        surface.open(0, pane::PaneSide::Flow, drawings::DrawingId(1), form());
        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &env(1));
        });
        assert!(
            !surface.is_open(),
            "the dialog does not follow the trader to another tab"
        );
    }

    /// An open dialog nobody touched survives the frame with the form intact
    /// — several fields long, and losing it on any quiet frame would make it
    /// unusable.
    #[test]
    fn an_untouched_dialog_survives_the_frame() {
        let ctx = egui::Context::default();
        let mut surface = StrategyPopupSurface::default();
        surface.open(0, pane::PaneSide::Flow, drawings::DrawingId(1), form());
        surface.form_mut().expect("the dialog is open").quantity = "3".to_owned();
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(0));
        });
        assert!(surface.is_open());
        assert_eq!(
            surface.form_mut().expect("still open").quantity.as_str(),
            "3"
        );
        assert!(response.arm_strategy.is_none());
        assert!(response.test_alert.is_none());
    }

    /// The host's refusal lands in the footer and the form stays exactly as
    /// it was — one corrected field from an arming that works.
    #[test]
    fn a_refused_arming_keeps_the_form_and_shows_why() {
        let mut surface = StrategyPopupSurface::default();
        surface.open(0, pane::PaneSide::Flow, drawings::DrawingId(1), form());
        surface.form_mut().expect("the dialog is open").quantity = "not a number".to_owned();
        surface.settle_arm(Err("a field does not parse".to_owned()));
        assert!(surface.is_open(), "a refusal does not close the dialog");
        assert_eq!(surface.error(), Some("a field does not parse"));
        assert_eq!(
            surface.form_mut().expect("still open").quantity.as_str(),
            "not a number"
        );
    }

    /// And an arming that worked closes it.
    #[test]
    fn an_accepted_arming_closes_the_dialog() {
        let mut surface = StrategyPopupSurface::default();
        surface.open(0, pane::PaneSide::Flow, drawings::DrawingId(1), form());
        surface.settle_arm(Ok(()));
        assert!(!surface.is_open());
    }

    /// The host may answer after the trader has already cancelled. An answer
    /// with nowhere to land is dropped, not a panic.
    #[test]
    fn an_answer_for_a_dialog_that_left_is_dropped() {
        let mut surface = StrategyPopupSurface::default();
        surface.settle_arm(Err("too late".to_owned()));
        assert!(!surface.is_open());
        assert_eq!(surface.error(), None);
    }
}
