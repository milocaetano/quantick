//! A headless consumer of the same linear protocol as the filesystem executor.
use quantick_workspace::bundle::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

type Trace = Rc<RefCell<Vec<String>>>;
struct Store {
    key: &'static str,
    included: bool,
    local: bool,
    trace: Trace,
}
impl BundleStore for Store {
    fn key(&self) -> &str {
        self.key
    }
    fn included(&self) -> bool {
        self.included
    }
    fn local_keys(&self) -> &[&str] {
        if self.local { &["machine"] } else { &[] }
    }
    fn validate_text(&self, text: &str) -> Result<(), SectionError> {
        self.trace
            .borrow_mut()
            .push(format!("validate {}", self.key));
        if text.contains("reject") {
            Err(SectionError::Malformed("literal rejection".into()))
        } else {
            Ok(())
        }
    }
}
fn registry(trace: &Trace) -> Vec<Store> {
    ["first", "second", "third", "fourth", "paper"]
        .into_iter()
        .map(|key| Store {
            key,
            included: key != "paper",
            local: key == "first",
            trace: trace.clone(),
        })
        .collect()
}
const SOURCE: &str = r#"version = 1
[sections.first]
shape = "round"
machine = "incoming"
[sections.second]
shape = "round"
[sections.third]
shape = "round"
[sections.fourth]
shape = "round"
[sections.paper]
account = "foreign"
"#;

struct Storage {
    live: BTreeMap<String, String>,
    temps: BTreeMap<String, String>,
    fail_stage: Option<usize>,
    fail_install: Option<usize>,
    fail_read: Option<usize>,
    trace: Trace,
}
impl Storage {
    fn new(trace: &Trace) -> Self {
        Self {
            live: ["first", "second", "third", "fourth", "paper"]
                .into_iter()
                .map(|key| {
                    (
                        key.into(),
                        "shape = \"square\"\nmachine = \"local\"\n".into(),
                    )
                })
                .collect(),
            temps: BTreeMap::new(),
            fail_stage: None,
            fail_install: None,
            fail_read: None,
            trace: trace.clone(),
        }
    }
    fn record(&self, text: String) {
        self.trace.borrow_mut().push(text);
    }
    fn stage(&mut self, index: usize, key: &str, text: &str) -> std::io::Result<()> {
        self.record(format!("stage {key}"));
        if self.fail_stage == Some(index) {
            self.temps.insert(key.into(), "partial failed write".into());
            return Err(std::io::Error::other("stage failure"));
        }
        self.temps.insert(key.into(), text.into());
        Ok(())
    }
    fn install(&mut self, index: usize, key: &str) -> std::io::Result<()> {
        self.record(format!("install {key}"));
        if self.fail_install == Some(index) {
            return Err(std::io::Error::other("rename failure"));
        }
        let text = self
            .temps
            .remove(key)
            .expect("all stores staged before install");
        self.live.insert(key.into(), text);
        Ok(())
    }
    fn execute<'stores>(
        &mut self,
        bundle: &Bundle,
        stores: &'stores [Store],
    ) -> Result<InstalledStores<'stores>, ImportFailure> {
        let mut step = ImportTransaction::begin(bundle, stores);
        loop {
            step = match step {
                ImportStep::Notice(notice) => {
                    self.record(format!("notice {}", notice.key()));
                    notice.acknowledge()
                }
                ImportStep::Prepare(prepare) => {
                    let index = prepare.store_index();
                    let key = stores[index].key;
                    self.record(format!("resolve {key}"));
                    let current = if prepare.needs_current_text() {
                        self.record(format!("read {key}"));
                        if self.fail_read == Some(index) {
                            CurrentText::Unavailable
                        } else {
                            self.live
                                .get(key)
                                .cloned()
                                .map_or(CurrentText::Unavailable, CurrentText::Available)
                        }
                    } else {
                        CurrentText::Unavailable
                    };
                    prepare.complete(current)
                }
                ImportStep::Stage(stage) => {
                    let index = stage.store_index();
                    let result = self.stage(index, stores[index].key, stage.text());
                    stage.complete(result)
                }
                ImportStep::Install(install) => {
                    let index = install.store_index();
                    let result = self.install(index, stores[index].key);
                    install.complete(result)
                }
                ImportStep::Cleanup(cleanup) => {
                    let key = stores[cleanup.store_index()].key;
                    self.record(format!("cleanup {key}"));
                    self.temps.remove(key);
                    cleanup.complete()
                }
                ImportStep::Finished(result) => return result,
            };
        }
    }
    fn capture(&mut self, stores: &[Store]) -> Result<Bundle, CaptureFailure> {
        let mut step = CaptureTransaction::begin("literal", stores);
        loop {
            step = match step {
                CaptureStep::Read(read) => {
                    let index = read.store_index();
                    let key = stores[index].key;
                    self.record(format!("resolve {key}"));
                    self.record(format!("read {key}"));
                    let result = if self.fail_read == Some(index) {
                        CaptureText::Failed("read failure".into())
                    } else {
                        self.live
                            .get(key)
                            .cloned()
                            .map_or(CaptureText::Missing, CaptureText::Text)
                    };
                    read.complete(result)
                }
                CaptureStep::Finished(result) => return result,
            };
        }
    }
}

// This compiles only if the returned receipt is independent of the local Bundle.
fn import_local<'stores>(
    storage: &mut Storage,
    stores: &'stores [Store],
) -> Result<InstalledStores<'stores>, ImportFailure> {
    let bundle: Bundle = toml::from_str(SOURCE).unwrap();
    storage.execute(&bundle, stores)
}

#[test]
fn receipt_outlives_local_bundle_and_every_stage_precedes_install() {
    let trace = Trace::default();
    let stores = registry(&trace);
    let mut storage = Storage::new(&trace);
    let paper = storage.live["paper"].clone();
    let receipt = import_local(&mut storage, &stores).unwrap();
    assert_eq!(receipt.keys(), ["first", "second", "third", "fourth"]);
    assert_eq!(
        *trace.borrow(),
        [
            "validate first",
            "validate second",
            "validate third",
            "validate fourth",
            "resolve first",
            "read first",
            "stage first",
            "resolve second",
            "stage second",
            "resolve third",
            "stage third",
            "resolve fourth",
            "stage fourth",
            "install first",
            "install second",
            "install third",
            "install fourth"
        ]
    );
    assert_eq!(storage.live["paper"], paper);
    let first: toml::Value = toml::from_str(&storage.live["first"]).unwrap();
    assert_eq!(first["machine"].as_str(), Some("local"));
    assert_eq!(first["shape"].as_str(), Some("round"));
    assert!(storage.temps.is_empty());
}

#[test]
fn notices_precede_first_invalid_store_but_unsupported_version_is_silent() {
    for version in [1, 99] {
        let trace = Trace::default();
        let stores = registry(&trace);
        let mut storage = Storage::new(&trace);
        let original = storage.live.clone();
        let text = format!(
            "version = {version}\n[sections.z_future]\nx = 1\n[sections.a_future]\nx = 1\n[sections.first]\nshape = \"round\"\n[sections.second]\nreject = true\n[sections.third]\nshape = \"round\"\n"
        );
        let bundle = toml::from_str(&text).unwrap();
        let result = storage.execute(&bundle, &stores).unwrap_err();
        if version == 1 {
            assert_eq!(
                result,
                ImportFailure::InvalidSection {
                    store_index: 1,
                    error: SectionError::Malformed("literal rejection".into())
                }
            );
            assert_eq!(
                *trace.borrow(),
                [
                    "notice a_future",
                    "notice z_future",
                    "validate first",
                    "validate second"
                ]
            );
        } else {
            assert_eq!(result, ImportFailure::Version(99));
            assert!(trace.borrow().is_empty());
        }
        assert_eq!(storage.live, original);
        assert!(storage.temps.is_empty());
    }
}

#[test]
fn failed_stage_cleans_only_previous_temps_and_leaves_no_installed_store() {
    let trace = Trace::default();
    let stores = registry(&trace);
    let mut storage = Storage::new(&trace);
    let original = storage.live.clone();
    storage.fail_stage = Some(2);
    assert_eq!(
        import_local(&mut storage, &stores).unwrap_err(),
        ImportFailure::Stage {
            store_index: 2,
            error: SectionError::Io {
                kind: std::io::ErrorKind::Other,
                message: "stage failure".into()
            }
        }
    );
    assert_eq!(
        *trace.borrow(),
        [
            "validate first",
            "validate second",
            "validate third",
            "validate fourth",
            "resolve first",
            "read first",
            "stage first",
            "resolve second",
            "stage second",
            "resolve third",
            "stage third",
            "cleanup first",
            "cleanup second"
        ]
    );
    assert_eq!(storage.live, original);
    assert_eq!(
        storage.temps,
        BTreeMap::from([("third".into(), "partial failed write".into())])
    );
    storage.fail_stage = None;
    assert_eq!(import_local(&mut storage, &stores).unwrap().keys().len(), 4);
    assert!(storage.temps.is_empty());
}

#[test]
fn second_and_fourth_rename_failure_keep_predecessors_and_later_temps_until_reimport() {
    for failed in [1, 3] {
        let trace = Trace::default();
        let stores = registry(&trace);
        let mut storage = Storage::new(&trace);
        let original = storage.live.clone();
        storage.fail_install = Some(failed);
        assert_eq!(
            import_local(&mut storage, &stores).unwrap_err(),
            ImportFailure::Install {
                store_index: failed,
                installed: failed,
                total: 4,
                error: SectionError::Io {
                    kind: std::io::ErrorKind::Other,
                    message: "rename failure".into(),
                },
            }
        );
        for (index, key) in ["first", "second", "third", "fourth"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(storage.live[key] != original[key], index < failed);
            assert_eq!(storage.temps.contains_key(key), index > failed);
        }
        assert_eq!(
            trace.borrow().last().unwrap(),
            &format!("cleanup {}", stores[failed].key)
        );
        assert_eq!(
            trace
                .borrow()
                .iter()
                .filter(|event| event.starts_with("install "))
                .count(),
            failed + 1
        );
        storage.fail_install = None;
        trace.borrow_mut().clear();
        assert_eq!(
            import_local(&mut storage, &stores).unwrap().keys(),
            ["first", "second", "third", "fourth"]
        );
        assert!(storage.temps.is_empty());
    }
}

#[test]
fn capture_terminates_before_later_reads_and_skips_missing_without_validation() {
    for failure in ["parse", "read"] {
        let trace = Trace::default();
        let stores = registry(&trace);
        let mut storage = Storage::new(&trace);
        if failure == "parse" {
            storage.live.insert("first".into(), "broken = [".into());
        } else {
            storage.fail_read = Some(0);
        }
        assert!(storage.capture(&stores).is_err());
        assert_eq!(*trace.borrow(), ["resolve first", "read first"]);
    }
    let trace = Trace::default();
    let stores = registry(&trace);
    let mut storage = Storage::new(&trace);
    storage.live.remove("first");
    storage.live.insert("second".into(), "reject = true".into());
    let bundle = storage.capture(&stores).unwrap();
    assert_eq!(bundle.len(), 3);
    assert_eq!(bundle.sections["second"]["reject"].as_bool(), Some(true));
    assert_eq!(
        *trace.borrow(),
        [
            "resolve first",
            "read first",
            "resolve second",
            "read second",
            "resolve third",
            "read third",
            "resolve fourth",
            "read fourth"
        ]
    );
}

#[test]
fn local_unavailable_malformed_or_absent_key_preserves_incoming_value() {
    for held in [None, Some("broken = ["), Some("shape = \"square\"")] {
        let trace = Trace::default();
        let stores = registry(&trace);
        let mut storage = Storage::new(&trace);
        if let Some(text) = held {
            storage.live.insert("first".into(), text.into());
        } else {
            storage.fail_read = Some(0);
        }
        import_local(&mut storage, &stores).unwrap();
        let first: toml::Value = toml::from_str(&storage.live["first"]).unwrap();
        assert_eq!(first["machine"].as_str(), Some("incoming"));
    }
}

#[test]
fn registration_alone_supports_a_second_store_and_empty_selection() {
    let trace = Trace::default();
    let stores = [Store {
        key: "external_plugin",
        included: true,
        local: false,
        trace: trace.clone(),
    }];
    let mut storage = Storage::new(&trace);
    let bundle =
        toml::from_str("version = 1\n[sections.external_plugin]\nshape = \"round\"\n").unwrap();
    assert_eq!(
        storage.execute(&bundle, &stores).unwrap().keys(),
        ["external_plugin"]
    );
    trace.borrow_mut().clear();
    let empty = toml::from_str("version = 1\n").unwrap();
    assert!(storage.execute(&empty, &stores).unwrap().keys().is_empty());
    assert!(trace.borrow().is_empty());
}
