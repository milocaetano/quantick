//! Theme persistence. A file is validated whole before replacing active state.

use std::io::{Read, Write};
use std::path::Path;

pub use quantick_appearance::Builtin;
use quantick_appearance::{Appearance, ApplyOutcome, ResolvedTheme, ThemeError};

pub const DEFAULT_SOURCE: &str = quantick_appearance::DARK;

pub const THEME_FILE: &str = "appearance.toml";

#[derive(Debug)]
pub enum AppearanceError {
    Invalid(ThemeError),
    Io(std::io::Error),
}

impl std::fmt::Display for AppearanceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "theme file: {error}"),
        }
    }
}

impl std::error::Error for AppearanceError {}

fn read_source(path: &Path) -> std::io::Result<String> {
    let mut source = String::new();
    std::fs::File::open(path)?
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_string(&mut source)?;
    Ok(source)
}

/// Missing files open Dark. Malformed or unreadable files are reported; callers
/// must not silently replace a user's invalid file with a default theme.
pub fn load(path: &Path) -> Result<Appearance, AppearanceError> {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Appearance::default());
        }
        Err(error) => return Err(AppearanceError::Io(error)),
    };
    check_source(&source)?;
    let mut state = Appearance::default();
    state.apply(&source).map_err(AppearanceError::Invalid)?;
    Ok(state)
}

/// The bundle registry uses the same full validation gate as a live change.
pub fn validate(source: &str) -> Result<(), String> {
    ResolvedTheme::parse(source)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Persist the validated candidate, then publish it. The host installs renderer
/// resources only after this succeeds. The gateway deduplicates keyed retries.
pub fn apply_and_save(
    state: &mut Appearance,
    path: &Path,
    source: &str,
) -> Result<ApplyOutcome, AppearanceError> {
    check_source(source)?;
    let mut candidate = state.clone();
    let outcome = candidate.apply(source).map_err(AppearanceError::Invalid)?;
    write_candidate(path, source).map_err(AppearanceError::Io)?;
    *state = candidate;
    Ok(outcome)
}

/// Appending keeps staging distinct even when the selected destination ends in .tmp.
/// create_new never truncates a pre-existing staging file owned by another writer.
fn write_candidate(path: &Path, source: &str) -> std::io::Result<()> {
    quantick_workspace::write_refusal::guard_write(path).map_err(std::io::Error::other)?;
    let mut name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("theme destination has no filename"))?
        .to_os_string();
    name.push(".quantick-appearance.tmp");
    let staging = path.with_file_name(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    let written = file
        .write_all(source.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| std::fs::rename(&staging, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

/// A bounded control document. Checked before parsing, including local callers.
pub const MAX_SOURCE_BYTES: usize = 65_536;

fn check_source(source: &str) -> Result<(), AppearanceError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(AppearanceError::Invalid(ThemeError::new(
            "source",
            "theme exceeds 65536 UTF-8 bytes",
        )));
    }
    Ok(())
}

/// The selected destination is host-owned and never accepted from remote input.
pub struct AppearanceStore {
    state: Appearance,
    path: std::path::PathBuf,
    origin: &'static str,
    durable: bool,
    load_failed: bool,
    inspection_revision: u64,
    choice_path: std::path::PathBuf,
}

impl std::ops::Deref for AppearanceStore {
    type Target = Appearance;
    fn deref(&self) -> &Appearance {
        &self.state
    }
}

impl AppearanceStore {
    pub fn open(path: std::path::PathBuf) -> Self {
        let choice_path = path.with_file_name(format!(
            "{}.choice.toml",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let path = if choice_path.exists() {
            choice_path.clone()
        } else {
            path
        };
        let (state, load_failed) = match load(&path) {
            Ok(state) => (state, false),
            Err(error) => {
                tracing::warn!(%error, "appearance could not be loaded; retaining Dark");
                (Appearance::default(), true)
            }
        };
        let durable = state.source().is_some();
        Self {
            state,
            path,
            origin: if durable {
                "selected_store"
            } else {
                "bundled_dark"
            },
            durable,
            load_failed,
            inspection_revision: 0,
            choice_path,
        }
    }
    /// Source and persistence change independently of paint. Inspection keys must
    /// not clone an entire TOML document merely to compare consecutive captures.
    pub fn inspection_revision(&self) -> u64 {
        self.inspection_revision
    }
    pub fn origin(&self) -> &'static str {
        self.origin
    }
    /// Last successful load/save, not a claim to poll externally edited files.
    pub fn durable(&self) -> bool {
        self.durable
    }
    pub fn load_failed(&self) -> bool {
        self.load_failed
    }
    pub fn writes_enabled(&self) -> bool {
        quantick_workspace::write_refusal::writes_refused().is_none()
    }
    /// A native choice never overwrites an authored input file. Once selected,
    /// the sibling becomes the active store for subsequent explicit saves too.
    pub fn apply_builtin(
        &mut self,
        builtin: quantick_appearance::Builtin,
        persist: bool,
    ) -> Result<ApplyOutcome, AppearanceError> {
        if !persist {
            return self.apply_candidate(builtin.source(), false);
        }
        let previous = std::mem::replace(&mut self.path, self.choice_path.clone());
        let result = self.apply_candidate(builtin.source(), true);
        if result.is_err() {
            self.path = previous;
        }
        result
    }
    pub fn apply_candidate(
        &mut self,
        source: &str,
        persist: bool,
    ) -> Result<ApplyOutcome, AppearanceError> {
        check_source(source)?;
        let origin = if persist { "selected_store" } else { "session" };
        let inspection_changed = self.state.source() != Some(source)
            || self.origin != origin
            || self.durable != persist
            || (persist && self.load_failed);
        let outcome = if persist {
            apply_and_save(&mut self.state, &self.path, source)?
        } else {
            self.state.apply(source).map_err(AppearanceError::Invalid)?
        };
        if inspection_changed {
            self.inspection_revision = self.inspection_revision.wrapping_add(1);
        }
        self.origin = origin;
        self.durable = persist;
        if persist {
            self.load_failed = false;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;
    use quantick_appearance::{DARK, LIGHT};

    #[test]
    fn builtin_choice_preserves_authored_source_and_roundtrips_active_destination() {
        let scratch = ScratchDir::new("appearance-choice");
        let path = scratch.path().join("community.toml");
        let authored = "version=1\nname='Light'\n[colors]\naccent='#AABBCC'";
        std::fs::write(&path, authored).unwrap();
        let mut store = AppearanceStore::open(path.clone());
        assert_eq!(
            store.builtin(),
            None,
            "names never establish builtin identity"
        );
        store.apply_builtin(Builtin::Light, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), authored);
        let mut reopened = AppearanceStore::open(path.clone());
        assert_eq!(reopened.builtin(), Some(Builtin::Light));
        assert!(reopened.durable());
        reopened.apply_builtin(Builtin::Dark, true).unwrap();
        assert_eq!(
            AppearanceStore::open(path.clone()).builtin(),
            Some(Builtin::Dark)
        );
        let custom = format!("# authored copy\n{LIGHT}");
        reopened.apply_candidate(&custom, true).unwrap();
        let final_state = AppearanceStore::open(path.clone());
        assert_eq!(final_state.builtin(), None);
        assert_eq!(final_state.source(), Some(custom.as_str()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), authored);
    }

    #[test]
    fn failed_builtin_save_preserves_source_destination_and_inspection() {
        let scratch = ScratchDir::new("appearance-choice-failure");
        let path = scratch.path().join(THEME_FILE);
        let mut store = AppearanceStore::open(path.clone());
        std::fs::create_dir(&store.choice_path).unwrap();
        assert!(store.apply_builtin(Builtin::Light, true).is_err());
        assert_eq!(store.builtin(), Some(Builtin::Dark));
        assert_eq!(store.inspection_revision(), 0);
        assert!(!store.durable());
        store.apply_candidate(LIGHT, true).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), LIGHT);
    }

    #[test]
    fn builtin_session_choice_never_creates_saved_preference() {
        let scratch = ScratchDir::new("appearance-choice-session");
        let path = scratch.path().join(THEME_FILE);
        let mut store = AppearanceStore::open(path.clone());
        store.apply_builtin(Builtin::Light, false).unwrap();
        assert_eq!(store.builtin(), Some(Builtin::Light));
        assert!(!store.choice_path.exists());
        assert_eq!(AppearanceStore::open(path).builtin(), Some(Builtin::Dark));
    }

    #[test]
    fn saves_off_refuses_builtin_preference_before_publication() {
        use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};
        let scratch = ScratchDir::new("appearance-choice-refused");
        let mut store = AppearanceStore::open(scratch.path().join(THEME_FILE));
        refuse_writes_on_this_thread(Some(WritesRefused {
            hooks: vec!["QUANTICK_UNKNOWN_TEST".into()],
        }));
        let result = store.apply_builtin(Builtin::Light, true);
        refuse_writes_on_this_thread(None);
        assert!(result.is_err());
        assert_eq!(store.builtin(), Some(Builtin::Dark));
        assert!(!store.choice_path.exists());
        assert_eq!(store.inspection_revision(), 0);
    }

    #[test]
    fn utf8_byte_boundary_is_shared_by_load_session_and_durable_apply() {
        let scratch = ScratchDir::new("appearance-bounds");
        let path = scratch.path().join(THEME_FILE);
        let prefix = "version = 1\nname = 'Bounds'\n#";
        let mut exact = format!(
            "{prefix}{}",
            "\u{00e9}".repeat((MAX_SOURCE_BYTES - prefix.len()) / 2)
        );
        while exact.len() < MAX_SOURCE_BYTES {
            exact.push(' ');
        }
        let mut store = AppearanceStore::open(path.clone());
        store.apply_candidate(&exact, true).unwrap();
        assert_eq!(load(&path).unwrap().source(), Some(exact.as_str()));
        let over = format!("{exact}\u{00e9}");
        assert!(
            over.chars().count() < MAX_SOURCE_BYTES,
            "runtime also enforces UTF-8 bytes"
        );
        assert!(store.apply_candidate(&over, false).is_err());
        assert!(store.apply_candidate(&over, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), exact);
        std::fs::write(&path, &over).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn external_tmp_destination_never_aliases_or_overwrites_staging() {
        let scratch = ScratchDir::new("appearance-tmp");
        let path = scratch.path().join("community.tmp");
        std::fs::write(&path, DARK).unwrap();
        let mut store = AppearanceStore::open(path.clone());
        store.apply_candidate(LIGHT, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
        let staging = scratch.path().join("community.tmp.quantick-appearance.tmp");
        assert!(!staging.exists());
        std::fs::write(&staging, "another writer").unwrap();
        assert!(store.apply_candidate(DARK, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
        assert_eq!(std::fs::read_to_string(&staging).unwrap(), "another writer");
        assert_eq!(store.source(), Some(LIGHT));
    }

    #[test]
    fn session_then_durable_same_source_writes_and_removed_destinations_are_repaired() {
        let scratch = ScratchDir::new("appearance-session-save");
        let path = scratch.path().join(THEME_FILE);
        let mut store = AppearanceStore::open(path.clone());
        store.apply_candidate(LIGHT, false).unwrap();
        let revision = store.revision();
        let inspection = store.inspection_revision();
        assert!(!path.exists());
        assert!(!store.durable());
        assert_eq!(store.origin(), "session");
        assert_eq!(
            store.apply_candidate(LIGHT, true).unwrap(),
            ApplyOutcome::Unchanged
        );
        assert!(store.durable());
        assert_eq!(store.inspection_revision(), inspection + 1);
        store.apply_candidate(LIGHT, true).unwrap();
        assert_eq!(store.inspection_revision(), inspection + 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
        std::fs::remove_file(&path).unwrap();
        store.apply_candidate(LIGHT, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
        let formatted = format!("# different formatting\n{LIGHT}");
        store.apply_candidate(&formatted, true).unwrap();
        assert_eq!(store.revision(), revision);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), formatted);
        let other = scratch.path().join("other.toml");
        let mut state = load(&path).unwrap();
        apply_and_save(&mut state, &other, &formatted).unwrap();
        assert_eq!(std::fs::read_to_string(other).unwrap(), formatted);
    }

    #[test]
    fn failed_candidates_and_disabled_same_source_save_preserve_all_state() {
        use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};
        let scratch = ScratchDir::new("appearance-owner-atomicity");
        let path = scratch.path().join(THEME_FILE);
        std::fs::write(&path, "old invalid file").unwrap();
        let mut store = AppearanceStore::open(path.clone());
        assert!(store.load_failed());
        store.apply_candidate(LIGHT, false).unwrap();
        let revision = store.revision();
        for source in [
            "version = 99\nname = 'bad'",
            "version = 1\nname = 'bad'\n[colors]\ncanvas = 'invalid'",
            "version = 1\nname = 'bad'\n[typography.body]\nsize = -1",
            "version = 1\nname = 'bad'\ncandle_fill_opacity = 1.01",
            "version = 1\nname = 'bad'\n[orderflow.colors]\nbuy = '#ffffff80'",
        ] {
            assert!(store.apply_candidate(source, true).is_err());
            assert_eq!(store.source(), Some(LIGHT));
            assert_eq!(store.revision(), revision);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "old invalid file");
        }
        assert!(
            store
                .apply_candidate(&" ".repeat(MAX_SOURCE_BYTES + 1), false)
                .is_err()
        );
        refuse_writes_on_this_thread(Some(WritesRefused {
            hooks: vec!["QUANTICK_UNKNOWN_TEST".into()],
        }));
        assert!(!store.writes_enabled());
        let result = store.apply_candidate(LIGHT, true);
        refuse_writes_on_this_thread(None);
        assert!(result.is_err());
        assert!(!store.durable());
        assert_eq!(store.origin(), "session");
        assert!(store.load_failed());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old invalid file");
        store.apply_candidate(LIGHT, true).unwrap();
        assert!(!store.load_failed());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
    }

    #[test]
    fn save_reload_keeps_the_same_resolved_theme_and_original_source() {
        let scratch = ScratchDir::new("appearance-roundtrip");
        let path = scratch.path().join(THEME_FILE);
        let mut state = load(&path).unwrap();
        assert_eq!(state.resolved(), &ResolvedTheme::default());
        apply_and_save(&mut state, &path, LIGHT).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored.resolved(), state.resolved());
        assert_eq!(restored.source(), Some(LIGHT));
        assert_eq!(
            apply_and_save(&mut state, &path, LIGHT).unwrap(),
            ApplyOutcome::Unchanged
        );
    }

    #[test]
    fn failed_validation_preserves_disk_memory_and_revision() {
        let scratch = ScratchDir::new("appearance-invalid");
        let path = scratch.path().join(THEME_FILE);
        let mut state = Appearance::default();
        apply_and_save(&mut state, &path, LIGHT).unwrap();
        let before = state.clone();
        assert!(matches!(
            apply_and_save(&mut state, &path, "version = 99\nname = 'Invalid'"),
            Err(AppearanceError::Invalid(_))
        ));
        assert_eq!(state.resolved(), before.resolved());
        assert_eq!(state.revision(), before.revision());
        assert_eq!(state.source(), before.source());
        assert_eq!(std::fs::read_to_string(path).unwrap(), LIGHT);
    }

    #[test]
    fn failed_install_preserves_memory_and_removes_temporary_file() {
        let scratch = ScratchDir::new("appearance-write-failure");
        let path = scratch.path().join(THEME_FILE);
        std::fs::create_dir(&path).unwrap();
        let mut state = Appearance::default();
        assert!(matches!(
            apply_and_save(&mut state, &path, LIGHT),
            Err(AppearanceError::Io(_))
        ));
        assert_eq!(state.resolved(), &ResolvedTheme::default());
        assert_eq!(state.revision(), 0);
        assert_eq!(state.source(), None);
        assert!(path.is_dir());
        assert!(
            !scratch
                .path()
                .join("appearance.toml.quantick-appearance.tmp")
                .exists()
        );
    }

    #[test]
    fn malformed_load_is_reported_without_repairing_the_users_file() {
        let scratch = ScratchDir::new("appearance-load-failure");
        let path = scratch.path().join(THEME_FILE);
        std::fs::write(&path, "invalid TOML").unwrap();
        assert!(matches!(load(&path), Err(AppearanceError::Invalid(_))));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid TOML");
    }

    #[test]
    fn saves_off_gate_refuses_the_entire_theme_change() {
        use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};
        let scratch = ScratchDir::new("appearance-refused");
        let path = scratch.path().join(THEME_FILE);
        let mut state = Appearance::default();
        std::fs::write(&path, DARK).unwrap();
        refuse_writes_on_this_thread(Some(WritesRefused {
            hooks: vec!["QUANTICK_UNKNOWN_TEST".into()],
        }));
        let result = apply_and_save(&mut state, &path, LIGHT);
        refuse_writes_on_this_thread(None);
        assert!(matches!(result, Err(AppearanceError::Io(_))));
        assert_eq!(state.resolved(), &ResolvedTheme::default());
        assert_eq!(std::fs::read_to_string(path).unwrap(), DARK);
    }
}
