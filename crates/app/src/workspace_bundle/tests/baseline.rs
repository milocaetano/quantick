//! Literal pre-extraction oracles for observable read/notice order.
use super::*;
use std::cell::RefCell;
use std::io::Write;
use std::sync::{Arc, Mutex};

fn path() -> PathBuf {
    panic!("fixture resolves its own paths")
}
fn validate(text: &str) -> Result<(), String> {
    tracing::info!("validator reached");
    if text.contains("reject") {
        Err("literal refusal".into())
    } else {
        Ok(())
    }
}
fn store(key: &'static str) -> CockpitStore {
    CockpitStore {
        key,
        file: key,
        path,
        validate,
        in_bundle: true,
        local_keys: &["machine"],
    }
}
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn baseline_unknown_notices_precede_validation_error_but_not_version_error() {
    // Register the notice and validator callsites before this test's
    // subscriber exists. tracing caches a callsite's interest when it is
    // first hit; while this test's subscriber is the only live one, a
    // parallel test hitting a fresh callsite computes that interest against
    // its own thread's (empty) dispatcher and caches "never", which would
    // leave this oracle reading an empty log. Registered first, the
    // callsites are recomputed against every live subscriber when this one
    // is installed.
    let warm: Bundle = toml::from_str(
        "version = 1
[sections.a_future]
x = 1
[sections.known]
reject = true
",
    )
    .unwrap();
    let _ = apply(&warm, &[store("known")], &|_| {
        panic!("validation must prevent all path resolution")
    });
    for version in [1, 99] {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let text = format!(
            "version = {version}\n[sections.z_future]\nx = 1\n[sections.a_future]\nx = 1\n[sections.known]\nreject = true\n"
        );
        let bundle: Bundle = toml::from_str(&text).unwrap();
        let stores = [store("known")];
        let error = tracing::subscriber::with_default(subscriber, || {
            apply(&bundle, &stores, &|_| {
                panic!("validation must prevent all path resolution")
            })
        })
        .unwrap_err();
        let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        if version == 1 {
            assert_eq!(error, "section \"known\" is not valid: literal refusal");
            let a = log.find("section=a_future").unwrap();
            let z = log.find("section=z_future").unwrap();
            let validation = log.find("validator reached").unwrap();
            assert!(a < z && z < validation, "{log}");
        } else {
            assert_eq!(error, "workspace file version 99 (this build reads 1)");
            assert!(log.is_empty(), "{log}");
        }
    }
}

#[test]
fn baseline_capture_stops_before_later_paths_and_never_validates_export() {
    let dir = crate::scratch::ScratchDir::new("bundle-incremental");
    let stores = [store("first"), store("later")];
    std::fs::write(dir.join("first"), "broken = [").unwrap();
    let calls = RefCell::new(Vec::new());
    let error = capture("literal", &stores, &|store| {
        calls.borrow_mut().push(store.key);
        assert_eq!(store.key, "first");
        dir.join(store.file)
    })
    .unwrap_err();
    assert!(error.starts_with(&format!("{} is not readable:", dir.join("first").display())));
    assert_eq!(*calls.borrow(), ["first"]);
    std::fs::remove_file(dir.join("first")).unwrap();
    std::fs::write(dir.join("later"), "reject = true\nmachine = \"local\"\n").unwrap();
    calls.borrow_mut().clear();
    let bundle = capture("literal", &stores, &|store| {
        calls.borrow_mut().push(store.key);
        dir.join(store.file)
    })
    .unwrap();
    assert_eq!(*calls.borrow(), ["first", "later"]);
    assert_eq!(bundle.len(), 1);
    assert_eq!(
        bundle.sections["later"].get("reject").unwrap().as_bool(),
        Some(true)
    );
    assert!(bundle.sections["later"].get("machine").is_none());
}

#[test]
fn baseline_local_parse_failure_and_absent_key_preserve_incoming_local_value() {
    let dir = crate::scratch::ScratchDir::new("bundle-local-fallback");
    let stores = [store("known")];
    let bundle: Bundle = toml::from_str(
        "version = 1\n[sections.known]\nshape = \"round\"\nmachine = \"incoming\"\n",
    )
    .unwrap();
    for current in ["broken = [", "shape = \"square\"\n"] {
        std::fs::write(dir.join("known"), current).unwrap();
        apply(&bundle, &stores, &|store| dir.join(store.file)).unwrap();
        let held: toml::Value =
            toml::from_str(&std::fs::read_to_string(dir.join("known")).unwrap()).unwrap();
        assert_eq!(held["machine"].as_str(), Some("incoming"));
        assert_eq!(held["shape"].as_str(), Some("round"));
    }
}
