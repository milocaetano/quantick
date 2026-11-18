use super::*;
use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};

#[test]
fn the_status_lines_name_the_file_the_count_and_the_reason() {
    let path = Path::new("cockpits").join(format!("scalp.{BUNDLE_EXTENSION}"));
    let shown = path.display();
    assert_eq!(
        exported_note(&path, 7, " (bubbles left out)"),
        format!("Workspace exported to {shown} — 7 settings groups (bubbles left out)")
    );
    assert_eq!(
        imported_note(&path, 7),
        "Workspace \"scalp\" opened — 7 settings groups restored",
        "an import names the file as the recent menu does"
    );
    assert_eq!(
        not_exported_note("disk full"),
        "Workspace not exported — disk full"
    );
    assert_eq!(
        not_opened_note("bad file"),
        "Workspace not opened — bad file"
    );
    let home = Path::new("Documents").join("quantick");
    let shown = home.display();
    assert_eq!(
        cockpit_home_note(Some(&home)),
        format!("Cockpit saved in {shown}")
    );
    assert!(cockpit_home_note(None).starts_with("This system reports no documents folder"));
}

/// DS7: a saves-off session exporting a workspace leaves no trace on
/// disk, not even the folder it would have written into.
#[test]
fn a_refused_export_creates_no_folder() {
    let scratch = crate::scratch::ScratchDir::new("refused-export");
    let folder = scratch.path().join("exports");
    let bundle = Bundle {
        version: FORMAT_VERSION,
        name: "refused".to_owned(),
        sections: Default::default(),
    };
    refuse_writes_on_this_thread(Some(WritesRefused {
        hooks: vec!["QUANTICK_UI_STATE".to_owned()],
    }));
    let refused = write(&folder.join("cockpit.qws"), &bundle);
    refuse_writes_on_this_thread(None);
    assert!(refused.is_err(), "a refused session writes nothing");
    assert!(!folder.exists(), "and creates no folder either");
}
