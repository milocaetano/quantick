//! Theme persistence. A file is validated whole before replacing active state.

use std::path::Path;

use quantick_appearance::{Appearance, ApplyOutcome, ResolvedTheme, ThemeError};

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

/// Missing files open Dark. Malformed or unreadable files are reported; callers
/// must not silently replace a user's invalid file with a default theme.
pub fn load(path: &Path) -> Result<Appearance, AppearanceError> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Appearance::default());
        }
        Err(error) => return Err(AppearanceError::Io(error)),
    };
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
/// resources only after this succeeds. Exact retries do not touch the disk.
pub fn apply_and_save(
    state: &mut Appearance,
    path: &Path,
    source: &str,
) -> Result<ApplyOutcome, AppearanceError> {
    if state.source() == Some(source) {
        return Ok(ApplyOutcome::Unchanged);
    }
    let mut candidate = state.clone();
    let outcome = candidate.apply(source).map_err(AppearanceError::Invalid)?;
    crate::bundle::write_atomically(path, source).map_err(AppearanceError::Io)?;
    *state = candidate;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;
    use quantick_appearance::{DARK, LIGHT};

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
        assert!(!path.with_extension("tmp").exists());
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
