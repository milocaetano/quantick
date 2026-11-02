//! The desk calls through a fake window: each reaches its act once, a write
//! that failed journals nothing, and Agent access never opens from a connection.

use quantick_control_schema::desk::READBACKS;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

#[derive(Default)]
struct FakeHost {
    section: Option<SettingsSection>,
    saves: usize,
    forgets: usize,
    exports: usize,
    imports: usize,
    reopen: bool,
    write_fails: bool,
}

impl DeskHost for FakeHost {
    fn desk(&mut self, act: DeskAct) -> Result<bool, ControlError> {
        Ok(match act {
            DeskAct::OpenSettings(section) => {
                self.section = Some(section);
                true
            }
            DeskAct::Save => {
                self.saves += 1;
                !self.write_fails
            }
            DeskAct::Forget => {
                self.forgets += 1;
                !self.write_fails
            }
            DeskAct::PickExport => {
                self.exports += 1;
                self.exports == 1
            }
            DeskAct::PickImport => {
                self.imports += 1;
                true
            }
            DeskAct::ReadReopen => self.reopen,
            DeskAct::SetReopen(reopen) => {
                self.reopen = if self.write_fails {
                    self.reopen
                } else {
                    reopen
                };
                !self.write_fails
            }
        })
    }
}

fn parts() -> (FakeHost, FakeAccess, ActorContext) {
    let actor = FakeWindow::assistant();
    (FakeHost::default(), FakeAccess::with_budget(0), actor)
}

#[test]
fn settings_open_shows_the_asked_section_and_display_when_none_is_named() {
    let (mut host, mut access, actor) = parts();
    let answer = open_settings(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["section"], "display");
    assert_eq!(host.section, Some(SettingsSection::Display));
    for (token, section) in [
        ("data", SettingsSection::Data),
        ("recording", SettingsSection::Recording),
    ] {
        let value = json!({ "section": token });
        let answer = open_settings(&mut host, &mut access, &actor, &value).unwrap();
        assert_eq!(answer["section"], token);
        assert_eq!(host.section, Some(section));
    }
}

#[test]
fn a_connection_cannot_open_the_switch_that_widens_its_own_authority() {
    let (mut host, mut access, actor) = parts();
    let value = json!({ "section": "agent_access" });
    let refusal = open_settings(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(refusal.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(
        refusal.context.details.unwrap()["code"],
        AGENT_ACCESS_REFUSED_CODE
    );
    assert_eq!(host.section, None, "nothing came up");
}

#[test]
fn a_save_that_reached_the_disk_is_journaled_and_one_that_did_not_is_not() {
    let (mut host, mut access, actor) = parts();
    let answer = save(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["written"], true);
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].kind.as_str(), DESK_SAVED_EVENT_KIND);

    host.write_fails = true;
    let answer = save(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["written"], false);
    assert_eq!(access.events.len(), 1, "no event claims the failed write");
    assert_eq!(host.saves, 2);
}

#[test]
fn forget_reaches_the_host_and_journals_what_it_wrote() {
    let (mut host, mut access, actor) = parts();
    let answer = forget(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["written"], true);
    assert_eq!(host.forgets, 1);
    assert_eq!(access.events[0].kind.as_str(), DESK_FORGOTTEN_EVENT_KIND);
}

#[test]
fn export_and_import_raise_the_dialog_and_name_which_one() {
    let (mut host, mut access, actor) = parts();
    let answer = export(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["opened"], true);
    let answer = import(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["opened"], true);
    assert_eq!((host.exports, host.imports), (1, 1));
    assert_eq!(access.events[0].payload["capability"], "desk.export");
    assert_eq!(access.events[1].payload["capability"], "desk.import");

    let answer = export(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(answer["opened"], false, "a dialog is already up");
    assert_eq!(access.events.len(), 2);
}

#[test]
fn a_path_is_refused_before_any_dialog() {
    let (mut host, mut access, actor) = parts();
    let value = json!({ "path": "C:/elsewhere.toml" });
    assert!(export(&mut host, &mut access, &actor, &value).is_err());
    assert!(import(&mut host, &mut access, &actor, &value).is_err());
    assert_eq!((host.exports, host.imports), (0, 0));
}

#[test]
fn reopen_set_moves_the_choice_once_and_a_repeat_writes_nothing() {
    let (mut host, mut access, actor) = parts();
    let value = json!({ "reopen": true });
    let answer = reopen(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["written"], true);
    assert!(host.reopen);
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].payload["reopen"], true);

    let answer = reopen(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(answer["changed"], false);
    assert_eq!(answer["written"], false);
    assert_eq!(access.events.len(), 1, "a repeat journals nothing");
}

#[test]
fn a_failed_reopen_write_journals_nothing_and_a_retry_writes_again() {
    let (mut host, mut access, actor) = parts();
    host.write_fails = true;
    let value = json!({ "reopen": true });
    let answer = reopen(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(answer["reopen"], false, "the old choice is still in force");
    assert_eq!(answer["changed"], false);
    assert_eq!(answer["written"], false);
    assert!(access.events.is_empty());

    host.write_fails = false;
    let answer = reopen(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(answer["written"], true);
    assert_eq!(access.events.len(), 1);
}

#[test]
fn every_call_has_a_readback() {
    let rows: Vec<_> = READBACKS.iter().map(|row| row.capability).collect();
    assert_eq!(
        rows,
        [
            "settings.open",
            "desk.save",
            "desk.export",
            "desk.import",
            "desk.forget",
            "desk.reopen_on_launch.set",
        ]
    );
}
