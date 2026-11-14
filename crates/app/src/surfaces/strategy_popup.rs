//! Paint the strategy form and emit field intents; bank IO stays host-owned.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::{
    audio::{AlertSound, SoundCategory},
    strategy_presets as presets, theme,
};
use eframe::egui;
use quantick_ui_model::{
    UpdateSession,
    strategy_popup::{Effects, Field, Intent, Model, State, TextField},
};

type Session<'a> = UpdateSession<'a, Model>;
pub(crate) type StrategyPopupSurface = super::Adapter<Model, View>;
const ARM_DIALOG_BODY_SCREEN_FRACTION: f32 = 0.45;
const ARM_DIALOG_MIN_BODY_PT: f32 = 200.0;
pub(crate) struct View {
    pub bank: presets::StrategyBank,
    initialized: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            bank: presets::StrategyBank::load_from(presets::default_path()),
            initialized: false,
        }
    }
}
fn form<'a>(session: &'a Session<'_>) -> &'a presets::StoredPreset {
    &session.state().popup().expect("painted open dialog").form
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        arm_strategy: effects.arm,
        test_alert: effects.test_alert,
        ..Default::default()
    }
}
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "strategy-popup"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    fn paint_updated(
        &mut self,
        session: &mut Session<'_>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        if session.state().popup().is_some_and(|popup| {
            popup.tab != env.active_tab
                || session.state().shares_available != env.counted_bar_sides.contains(&popup.side)
        }) || session.state().alert_failure.as_deref() != env.alert_failure
        {
            session.update(Intent::Observe {
                active_tab: env.active_tab,
                counted_bar_sides: env.counted_bar_sides.to_vec(),
                alert_failure: env.alert_failure.map(str::to_owned),
            });
        }
        if !session.state().is_open() {
            return;
        }
        if !self.initialized {
            session.update(Intent::BankSnapshot(presets::snapshot(&self.bank)));
            self.initialized = true;
        }
        session.with_local_effects(
            &mut |mut effects: Effects| {
                let receipt = effects
                    .bank
                    .take()
                    .map(|effect| presets::apply(&mut self.bank, effect));
                (effects, receipt)
            },
            |session| paint_popup(session, target.context()),
        );
    }
}
fn paint_popup(session: &mut Session<'_>, ctx: &egui::Context) {
    let mut open = true;
    let max_body =
        (ctx.screen_rect().height() * ARM_DIALOG_BODY_SCREEN_FRACTION).max(ARM_DIALOG_MIN_BODY_PT);
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
                    preset_row(ui, session);
                    trade_fields(ui, session);
                    ui.separator();
                    alarm_controls(ui, session);
                    ui.separator();
                    bank_row(ui, session);
                });
            footer(ui, session);
        });
    if !open {
        session.update(Intent::Cancel);
    }
}

fn preset_row(ui: &mut egui::Ui, session: &mut Session<'_>) {
    ui.horizontal(|ui| {
        ui.label("preset");
        let current = session
            .state()
            .popup()
            .unwrap()
            .preset_choice
            .as_deref()
            .unwrap_or("custom");
        egui::ComboBox::from_id_salt("strategy_preset_pick")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for index in 0..session.state().rows().len() {
                    let name = &session.state().rows()[index].0;
                    let picked =
                        session.state().popup().unwrap().preset_choice.as_ref() == Some(name);
                    if ui.selectable_label(picked, name).clicked() {
                        session.update(Intent::SelectPreset(name.clone()));
                    }
                }
            });
    });
}
fn bank_row(ui: &mut egui::Ui, session: &mut Session<'_>) {
    ui.horizontal(|ui| {
        let mut name = session.state().popup().unwrap().save_name.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("preset name")
                    .desired_width(140.0),
            )
            .changed()
        {
            session.update(Intent::SaveName(name));
        }
        let can_save = !session.state().popup().unwrap().save_name.trim().is_empty();
        if ui
            .add_enabled(can_save, egui::Button::new("Save preset"))
            .clicked()
        {
            session.update(Intent::SavePreset);
        }
        if session.state().popup().unwrap().preset_choice.is_some()
            && ui
                .button("Delete preset")
                .on_hover_text("remove it from the bank; the form keeps its values")
                .clicked()
        {
            session.update(Intent::DeletePreset);
        }
    });
}
fn footer(ui: &mut egui::Ui, session: &mut Session<'_>) {
    if let Some(error) = &session.state().popup().unwrap().error {
        ui.colored_label(theme::get(ui.ctx()).sell(), error);
    }
    ui.separator();
    let mut intent = None;
    ui.horizontal(|ui| {
        if ui.button("Arm").clicked() {
            intent = Some(Intent::Submit);
        }
        if ui.button("Cancel").clicked() {
            intent = Some(Intent::Cancel);
        }
    });
    if let Some(intent) = intent {
        session.update(intent);
    }
}
fn alarm_controls(ui: &mut egui::Ui, session: &mut Session<'_>) {
    let drop_sound_list_open = session.state().sound_picker_pending();
    session.update(Intent::AlarmSection);
    let mut alarm = form(session).alarm;
    if ui
        .checkbox(&mut alarm, "alarm on signal bar")
        .on_hover_text(
            "play a sound the moment this strategy's signal happens — the trigger \
                 fires on your side, inside the region. It is the *signal* that alarms, \
                 not the order: with the share option below, you hear it before the bar \
                 closes and before any order could be placed, which is the time you need \
                 to act on another platform.",
        )
        .changed()
    {
        session.update(Intent::Edit(Field::Alarm(alarm)));
    }
    if !form(session).alarm {
        session.update(Intent::Edit(Field::Alarm(false)));
        return;
    }
    when_rows(session, ui, session.state().shares_available);
    repeat_row(session, ui);
    // Preserve the pre-selection sound and pre-cut length used by this frame's Test.
    let current = AlertSound::from_token(&form(session).alarm_sound).unwrap_or_default();
    sound_row(session, ui, current, drop_sound_list_open);
    cut_row(session, ui, current);
    let mut only = form(session).alarm_only;
    if ui
        .checkbox(&mut only, "alarm only \u{2014} never place an order")
        .on_hover_text(
            "the instance watches, judges and alarms, and places nothing. For a \
                 trader who executes elsewhere: a simulated position they never meant \
                 to take would occupy the account and silence the next signal.",
        )
        .changed()
    {
        session.update(Intent::Edit(Field::AlarmOnly(only)));
    }
    if let Some(reason) = &session.state().alert_failure {
        ui.colored_label(
            theme::get(ui.ctx()).sell(),
            format!("no sound was played: {reason}"),
        );
    }
}
fn when_rows(session: &mut Session<'_>, ui: &mut egui::Ui, shares_available: bool) {
    let mut alarm_when = form(session).alarm_when.clone();
    let mut alarm_share_percent = form(session).alarm_share_percent;

    ui.horizontal(|ui| {
        ui.label("when:");
        let on_close = alarm_when != "share";
        if ui
            .selectable_label(on_close, "bar closes")
            .on_hover_text("the same instant the strategy itself judges")
            .clicked()
        {
            alarm_when = "on_close".to_owned();
        }
        let share_button = ui.add_enabled(
            shares_available,
            egui::SelectableLabel::new(!on_close, "part-way through the bar"),
        );
        if share_button.clicked() {
            alarm_when = "share".to_owned();
        }
        if !shares_available {
            share_button.on_hover_text(
                "this bar rule closes on a condition rather than on a count, so there \
                     is no share of it to wait for — the alarm speaks at the close",
            );
        }
    });
    if alarm_when == "share" {
        ui.horizontal(|ui| {
            ui.label("from");
            ui.add(
                egui::DragValue::new(&mut alarm_share_percent)
                    .range(presets::MIN_ALARM_SHARE_PERCENT..=presets::MAX_ALARM_SHARE_PERCENT),
            );
            ui.label("% of the bar onward").on_hover_text(
                "on a 2000-tick chart at 70%, the alarm starts judging past tick \
                     1400. The bar is still moving, so the signal is marked \"preview\" \
                     — and if it stops qualifying before the close, the chart says so.",
            );
        });
    }
    if alarm_when != form(session).alarm_when {
        session.update(Intent::Edit(Field::AlarmWhen(alarm_when == "share")));
    }
    if alarm_share_percent != form(session).alarm_share_percent {
        session.update(Intent::Edit(Field::AlarmShare(alarm_share_percent)));
    }
}
fn repeat_row(session: &mut Session<'_>, ui: &mut egui::Ui) {
    let mut alarm_repeat = form(session).alarm_repeat.clone();
    let mut alarm_cooldown_secs = form(session).alarm_cooldown_secs;

    ui.horizontal(|ui| {
        ui.label("repeat:");
        let once = alarm_repeat != "cooldown";
        if ui
            .selectable_label(once, "once per bar")
            .on_hover_text("one sound per bar, however many prints agree")
            .clicked()
        {
            alarm_repeat = "once_per_bar".to_owned();
        }
        if ui.selectable_label(!once, "every").clicked() {
            alarm_repeat = "cooldown".to_owned();
        }
        if alarm_repeat == "cooldown" {
            ui.add(
                egui::DragValue::new(&mut alarm_cooldown_secs)
                    .range(presets::MIN_ALARM_COOLDOWN_SECS..=presets::MAX_ALARM_COOLDOWN_SECS),
            );
            ui.label("s").on_hover_text(
                "counted across bars, not reset by one closing — the rule for a \
                     trader who wants a reminder rather than one notice",
            );
        }
    });
    if alarm_repeat != form(session).alarm_repeat {
        session.update(Intent::Edit(Field::AlarmRepeat(alarm_repeat == "cooldown")));
    }
    if alarm_cooldown_secs != form(session).alarm_cooldown_secs {
        session.update(Intent::Edit(Field::AlarmCooldown(alarm_cooldown_secs)));
    }
}
fn sound_row(
    session: &mut Session<'_>,
    ui: &mut egui::Ui,
    current: AlertSound,
    drop_sound_list_open: bool,
) {
    let mut alarm_sound = form(session).alarm_sound.clone();

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
                            alarm_sound = sound.token().to_owned();
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
            session.update(Intent::TestSound { current });
        }
    });
    if alarm_sound != form(session).alarm_sound {
        session.update(Intent::Edit(Field::AlarmSound(
            AlertSound::from_token(&alarm_sound).unwrap_or_default(),
        )));
    }
}
fn cut_row(session: &mut Session<'_>, ui: &mut egui::Ui, current: AlertSound) {
    ui.horizontal(|ui| {
        let mut cut = form(session).alarm_play_secs.is_some();
        if ui
            .checkbox(&mut cut, "stop after")
            .on_hover_text(
                "cut the sound here rather than letting it run to its end — a nature \
                     clip runs for minutes, and an alarm that outstays its news is one the \
                     trader learns to talk over. Off, the sound plays whole.",
            )
            .changed()
        {
            session.update(Intent::Edit(Field::AlarmCut(cut)));
        }
        if let Some(mut secs) = form(session).alarm_play_secs {
            ui.add(
                egui::DragValue::new(&mut secs)
                    .range(presets::MIN_ALARM_PLAY_SECS..=presets::MAX_ALARM_PLAY_SECS),
            );
            if Some(secs) != form(session).alarm_play_secs {
                session.update(Intent::Edit(Field::AlarmSeconds(secs)));
            }
            ui.label("s");
            if !current.can_be_cut() {
                // Honest rather than silent: the cut is stored and
                // applies the moment a clip is picked, but this sound
                // is one beep the operating system plays whole.
                ui.weak("(a system sound is one beep — the cut applies to the clips)");
            }
        }
    });
}
fn trade_fields(ui: &mut egui::Ui, session: &mut Session<'_>) {
    egui::Grid::new("strategy_trade_fields")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("Side");
            ui.horizontal(|ui| {
                let buy = form(session).side == "buy";
                if ui.selectable_label(buy, "BUY").clicked() {
                    session.update(Intent::Edit(Field::Side(true)));
                }
                if ui.selectable_label(!buy, "SELL").clicked() {
                    session.update(Intent::Edit(Field::Side(false)));
                }
            });
            ui.end_row();
            for (label, field) in [
                ("Quantity", TextField::Quantity),
                ("Minimum body / average", TextField::MinFactor),
                ("Maximum body / average", TextField::MaxFactor),
                ("Minimum candle (pts)", TextField::MinRange),
                ("TP / range ahead", TextField::TpMult),
                ("SL / range behind", TextField::SlMult),
            ] {
                ui.label(label);
                let mut text = field.read(form(session)).to_owned();
                if ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(70.0))
                    .changed()
                {
                    session.update(Intent::Edit(Field::Text(field, text)));
                }
                ui.end_row();
            }
            ui.label("Average body window");
            let mut window = form(session).window;
            if ui
                .add(
                    egui::DragValue::new(&mut window)
                        .range(1..=crate::strategy_presets::MAX_FORCE_WINDOW),
                )
                .changed()
            {
                session.update(Intent::Edit(Field::Window(window)));
            }
            ui.end_row();
        });
    ui.label("The candle floor includes wicks. Zero disables the floor or a projected leg.");
    let mut auto = form(session).rearm == "auto";
    if ui
        .checkbox(&mut auto, "re-arm automatically after the operation closes")
        .on_hover_text("off = one shot per arming, the over-fire guard")
        .changed()
    {
        session.update(Intent::Edit(Field::Rearm(auto)));
    }
    let mut retest = form(session).on_break == "retest_limit";
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
        session.update(Intent::Edit(Field::OnBreak(retest)));
    }
}

#[cfg(test)]
#[path = "strategy_popup/tests/mod.rs"]
mod tests;
