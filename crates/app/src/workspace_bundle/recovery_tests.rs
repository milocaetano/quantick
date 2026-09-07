//! Disk oracles for the production import's per-file recovery boundary.

use std::cell::RefCell;
use std::fs;
use std::ops::Range;

use super::*;
use crate::scratch::ScratchDir;
use crate::store_home::COCKPIT_STORES;

const LOCAL_UI: &str = r#"version = 1
active_tab = 0
tabs = []
window = [800.0, 600.0]
save_on_exit = false
replay_folder = "C:/local-tapes"
recent_workspaces = ["C:/local.qws.toml"]
saved = [{ name = "local bookmark", active_tab = 0, tabs = [] }]
"#;
const IMPORTED_UI: &str = r#"version = 1
active_tab = 0
tabs = []
window = [1200.0, 900.0]
save_on_exit = true
replay_folder = "D:/foreign-tapes"
recent_workspaces = ["D:/foreign.qws.toml"]
saved = [{ name = "foreign bookmark", active_tab = 0, tabs = [] }]
"#;
// An independent oracle: imported arrangement plus this machine's four keys.
const RECOVERED_UI: &str = r#"version = 1
active_tab = 0
tabs = []
window = [1200.0, 900.0]
save_on_exit = false
replay_folder = "C:/local-tapes"
recent_workspaces = ["C:/local.qws.toml"]
saved = [{ name = "local bookmark", active_tab = 0, tabs = [] }]
"#;
const IMPORTED_LAYERS: &str = "version = 1\n[layers]\ngrid = true\nheatmap = false\n";
const IMPORTED_LAYOUTS: &str =
    "version = 1\nactive = 1\nnext_id = 2\n[[layouts]]\nid = 1\nname = \"Imported\"\n";
const IMPORTED_SYMBOLS: &str = "version = 1\n[feeds]\nbinance = [\"ETHUSDT\"]\n";
const SIDECAR: &[u8] = b"opaque local paper sidecar\0account=local\r\n";

struct StoreCase {
    key: &'static str,
    original: &'static str,
    incoming: &'static str,
    recovered: &'static str,
}

// Deliberately not alphabetical: installation must follow this registry order.
const CASES: &[StoreCase] = &[
    StoreCase {
        key: "ui_state",
        original: LOCAL_UI,
        incoming: IMPORTED_UI,
        recovered: RECOVERED_UI,
    },
    StoreCase {
        key: "chart_layers",
        original: "version = 1\n[layers]\ngrid = false\nheatmap = true\n",
        incoming: IMPORTED_LAYERS,
        recovered: IMPORTED_LAYERS,
    },
    StoreCase {
        key: "foreign_store",
        original: "shape = \"square\"\nmachine_tag = \"local\"\n",
        incoming: "shape = \"round\"\nmachine_tag = \"foreign\"\n",
        recovered: "shape = \"round\"\nmachine_tag = \"local\"\n",
    },
    StoreCase {
        key: "layouts",
        original: "version = 1\nactive = 1\nnext_id = 2\n[[layouts]]\nid = 1\nname = \"Local\"\n",
        incoming: IMPORTED_LAYOUTS,
        recovered: IMPORTED_LAYOUTS,
    },
    StoreCase {
        key: "symbols",
        original: "version = 1\n[feeds]\nbinance = [\"BTCUSDT\"]\n",
        incoming: IMPORTED_SYMBOLS,
        recovered: IMPORTED_SYMBOLS,
    },
];

fn foreign_path() -> PathBuf {
    panic!("the fixture must resolve only its owned scratch paths")
}

fn validate_foreign(text: &str) -> Result<(), String> {
    let value: toml::Value = toml::from_str(text).map_err(|error| error.to_string())?;
    match value.get("shape").and_then(toml::Value::as_str) {
        Some("round" | "square") => Ok(()),
        _ => Err("foreign store needs a recognized shape".to_owned()),
    }
}

fn registered_store(key: &str) -> CockpitStore {
    if key == "foreign_store" {
        return CockpitStore {
            key: "foreign_store",
            env: "QUANTICK_FOREIGN_STORE",
            file: "foreign-store.toml",
            path: foreign_path,
            validate: validate_foreign,
            in_bundle: true,
            local_keys: &["machine_tag"],
        };
    }
    let store = COCKPIT_STORES
        .iter()
        .find(|store| store.key == key)
        .unwrap();
    CockpitStore {
        key: store.key,
        env: store.env,
        file: store.file,
        path: store.path,
        validate: store.validate,
        in_bundle: store.in_bundle,
        local_keys: store.local_keys,
    }
}

fn value(text: &str) -> toml::Value {
    toml::from_str(text).expect("the independent fixture is valid TOML")
}

fn recovered_bytes(case: &StoreCase) -> Vec<u8> {
    toml::to_string_pretty(&value(case.recovered))
        .unwrap()
        .into_bytes()
}

struct Fixture {
    live: ScratchDir,
    stores: Vec<CockpitStore>,
    bundle: Bundle,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let source = ScratchDir::new(&format!("{label}-source"));
        let live = ScratchDir::new(&format!("{label}-live"));
        let mut stores: Vec<_> = CASES
            .iter()
            .map(|case| registered_store(case.key))
            .collect();
        stores.push(registered_store("paper_state"));
        for (case, store) in CASES.iter().zip(&stores) {
            for text in [case.original, case.incoming, case.recovered] {
                (store.validate)(text).expect("fixture setup uses valid real store sections");
            }
            fs::write(source.join(store.file), case.incoming).unwrap();
            fs::write(live.join(store.file), case.original).unwrap();
            assert_ne!(case.original.as_bytes(), recovered_bytes(case));
        }
        fs::write(source.join("paper-state.toml"), b"foreign account").unwrap();
        fs::write(live.join("paper-state.toml"), SIDECAR).unwrap();
        let mut bundle = capture(label, &stores, &|store| source.join(store.file)).unwrap();
        assert_eq!(bundle.len(), CASES.len());
        assert!(!bundle.sections.contains_key("paper_state"));
        for store in &stores {
            if let Some(section) = bundle.sections.get(store.key) {
                for key in store.local_keys {
                    assert!(section.get(*key).is_none(), "capture excludes {key}");
                }
            }
        }
        // Even an explicitly supplied excluded section must never touch paper.
        bundle
            .sections
            .insert("paper_state".to_owned(), value("version = 99\n"));
        let file = source.join("recovery.qws.toml");
        write(&file, &bundle).unwrap();
        let reread = read(&file).unwrap();
        assert_eq!(reread, bundle);
        Self {
            live,
            stores,
            bundle: reread,
        }
    }

    fn assert_live(&self, label: &str, installed: usize) {
        for (index, (case, store)) in CASES.iter().zip(&self.stores).enumerate() {
            let bytes = fs::read(self.live.join(store.file)).unwrap();
            let expected = if index < installed {
                recovered_bytes(case)
            } else {
                case.original.as_bytes().to_vec()
            };
            assert_eq!(bytes, expected, "{label}: live {}", store.key);
            let persisted = value(std::str::from_utf8(&bytes).unwrap());
            for key in store.local_keys {
                assert_eq!(
                    persisted.get(*key),
                    value(case.original).get(*key),
                    "{label}: {key}"
                );
            }
            println!(
                "{label}: live {} = {:?}",
                store.key,
                String::from_utf8(bytes).unwrap()
            );
        }
        assert_eq!(
            fs::read(self.live.join("paper-state.toml")).unwrap(),
            SIDECAR
        );
        assert!(!self.live.join("paper-state.importing").exists());
        println!("{label}: paper sidecar unchanged = {SIDECAR:?}");
    }

    fn assert_staged(&self, label: &str, remaining: Range<usize>) {
        for (index, (case, store)) in CASES.iter().zip(&self.stores).enumerate() {
            let temp = self.live.join(store.file).with_extension("importing");
            if remaining.contains(&index) {
                let bytes = fs::read(&temp).unwrap();
                assert_eq!(
                    bytes,
                    recovered_bytes(case),
                    "{label}: staged {}",
                    store.key
                );
                println!(
                    "{label}: staged {} = {:?}",
                    store.key,
                    String::from_utf8(bytes).unwrap()
                );
            } else {
                assert!(!temp.exists(), "{label}: {} must be absent", temp.display());
                println!("{label}: staged {} absent", store.key);
            }
        }
    }

    fn recover_and_repeat(&self) {
        for label in ["ordinary recovery", "ordinary repeat"] {
            let written = apply(&self.bundle, &self.stores, &|store| {
                self.live.join(store.file)
            })
            .expect("ordinary production import completes the same bundle");
            assert_eq!(
                written,
                [
                    "ui_state",
                    "chart_layers",
                    "foreign_store",
                    "layouts",
                    "symbols"
                ]
            );
            self.assert_live(label, CASES.len());
            self.assert_staged(label, CASES.len()..CASES.len());
        }
    }
}

fn installation_case(label: &str, fail_at: Option<usize>) {
    let fixture = Fixture::new(label);
    fixture.assert_live("before import", 0);
    fixture.assert_staged("before import", 0..0);
    let operations = RefCell::new(Vec::new());
    let mut attempted = 0;
    let result = apply_with_rename(
        &fixture.bundle,
        &fixture.stores,
        &|store| {
            operations.borrow_mut().push(format!("stage {}", store.key));
            fixture.live.join(store.file)
        },
        |temp, live| {
            let store = &fixture.stores[attempted];
            assert_eq!(live, fixture.live.join(store.file));
            assert_eq!(temp, live.with_extension("importing"));
            let step = format!("before rename {}", attempted + 1);
            fixture.assert_live(&step, attempted);
            fixture.assert_staged(&step, attempted..CASES.len());
            operations
                .borrow_mut()
                .push(format!("rename {}", store.key));
            println!(
                "rename {}: {} -> {}",
                attempted + 1,
                temp.display(),
                live.display()
            );
            attempted += 1;
            if fail_at == Some(attempted) {
                Err(std::io::Error::other(format!(
                    "injected rename failure {attempted}"
                )))
            } else {
                fs::rename(temp, live)
            }
        },
    );
    let expected: Vec<_> = CASES
        .iter()
        .map(|case| format!("stage {}", case.key))
        .chain(
            CASES
                .iter()
                .take(fail_at.unwrap_or(CASES.len()))
                .map(|case| format!("rename {}", case.key)),
        )
        .collect();
    assert_eq!(*operations.borrow(), expected);
    println!("operation order: {:?}", operations.borrow());
    if let Some(failed) = fail_at {
        let error = result.expect_err("the injected installation rename must fail");
        let path = fixture.live.join(fixture.stores[failed - 1].file);
        assert_eq!(
            error,
            format!(
                "replaced {} of 5 settings groups, then {} failed: injected rename failure {failed}. Open the file again to finish.",
                failed - 1,
                path.display()
            )
        );
        println!("returned error: {error}");
        fixture.assert_live("after partial failure", failed - 1);
        // Successful renames consume their temp files; the failed temp is
        // removed; later temps survive until ordinary re-import stages again.
        fixture.assert_staged("after partial failure", failed..CASES.len());
    } else {
        assert_eq!(
            result.unwrap(),
            [
                "ui_state",
                "chart_layers",
                "foreign_store",
                "layouts",
                "symbols"
            ]
        );
        fixture.assert_live("after success", CASES.len());
        fixture.assert_staged("after success", CASES.len()..CASES.len());
    }
    fixture.recover_and_repeat();
}

#[test]
fn second_rename_failure_recovers_through_ordinary_import() {
    installation_case("second-rename-recovery", Some(2));
}

#[test]
fn fourth_rename_failure_recovers_through_ordinary_import() {
    installation_case("fourth-rename-recovery", Some(4));
}

#[test]
fn successful_install_stages_every_group_before_renaming() {
    installation_case("normal-import-order", None);
}

#[test]
fn invalid_section_reaches_no_staging_or_rename() {
    let mut fixture = Fixture::new("invalid-import-recovery");
    fixture
        .bundle
        .sections
        .insert("chart_layers".to_owned(), value("version = 99\n"));
    let error = apply_with_rename(
        &fixture.bundle,
        &fixture.stores,
        &|_| panic!("invalid sections must fail before any staging path is resolved"),
        |_, _| panic!("invalid sections must fail before installation"),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "section \"chart_layers\" is not valid: chart-layers format version 99 (this build reads 1)"
    );
    println!("returned validation error: {error}");
    fixture.assert_live("after validation failure", 0);
    fixture.assert_staged("after validation failure", 0..0);
}

#[test]
fn stage_write_failure_cleans_earlier_temps_without_installing() {
    let fixture = Fixture::new("stage-write-recovery");
    let blocked = fixture
        .live
        .join(fixture.stores[3].file)
        .with_extension("importing");
    fs::create_dir(&blocked).unwrap();
    let staged = RefCell::new(Vec::new());
    let error = apply_with_rename(
        &fixture.bundle,
        &fixture.stores,
        &|store| {
            staged.borrow_mut().push(store.key);
            fixture.live.join(store.file)
        },
        |_, _| panic!("a failed real staging write must prevent every rename"),
    )
    .unwrap_err();
    assert_eq!(
        *staged.borrow(),
        ["ui_state", "chart_layers", "foreign_store", "layouts"]
    );
    assert!(error.starts_with(&format!(
        "could not stage {}:",
        fixture.live.join("layouts.toml").display()
    )));
    assert!(error.ends_with(". Nothing was changed."));
    println!("returned real staging error: {error}");
    fixture.assert_live("after staging failure", 0);
    assert!(
        blocked.is_dir(),
        "the pre-existing blocker remains owned by the fixture"
    );
    assert_eq!(fs::read_dir(&blocked).unwrap().count(), 0);
    fs::remove_dir(&blocked).unwrap();
    fixture.assert_staged("after removing owned blocker", 0..0);
    fixture.recover_and_repeat();
}
