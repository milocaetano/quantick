//! Why a session writes no store: the typed answer the application's store
//! guard gives every refused write (decision DS7). Headless, so the refusal
//! is a value a caller matches on, not a sentence it parses.

/// Why a session writes no store (decision DS7): the `QUANTICK_*` names set
/// at launch that this build does not read. Typed, so a caller tells a
/// refusal from an I/O failure and a reader lists the names rather than
/// parsing a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritesRefused {
    /// The unread names, sorted and unique.
    pub hooks: Vec<String>,
}

impl std::fmt::Display for WritesRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "saving is off: {} set, which this build does not read \
             (capture hooks need a --features harness build)",
            self.hooks.join(", ")
        )
    }
}

impl std::error::Error for WritesRefused {}

impl From<WritesRefused> for String {
    fn from(refused: WritesRefused) -> Self {
        refused.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_every_unread_hook_and_the_build_that_reads_them() {
        let refused = WritesRefused {
            hooks: vec![
                "QUANTICK_LAYOUTS".to_owned(),
                "QUANTICK_UI_STATE".to_owned(),
            ],
        };
        assert_eq!(
            String::from(refused),
            "saving is off: QUANTICK_LAYOUTS, QUANTICK_UI_STATE set, which this build \
             does not read (capture hooks need a --features harness build)"
        );
    }
}
