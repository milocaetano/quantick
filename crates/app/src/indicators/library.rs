//! The script library: where `.pine` sources come from.
//!
//! The library itself — the embedded starter scripts and the folder scan —
//! lives in `quantick-stores`. The window resolves the folder: from
//! `QUANTICK_INDICATORS_DIR`, falling back to `./indicators` beside the
//! working directory — the same precedence spirit as `QUANTICK_CONFIG`.

use std::path::PathBuf;

#[cfg(test)]
pub(crate) use quantick_stores::script_library::EMBEDDED_SCRIPTS;
pub(crate) use quantick_stores::script_library::ScriptLibrary;

/// Environment override for the scripts folder.
pub(crate) const INDICATORS_DIR_ENV: &str = "QUANTICK_INDICATORS_DIR";
/// Default scripts folder, relative to the working directory.
const DEFAULT_DIR: &str = "indicators";

/// The library over the folder this launch resolves.
pub(crate) fn scan() -> ScriptLibrary {
    let dir = std::env::var(INDICATORS_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_DIR));
    ScriptLibrary::open(&dir)
}

crate::hooks::declare_hooks!["QUANTICK_INDICATORS_DIR"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scripts_compile_against_the_dialect() {
        for (name, source) in EMBEDDED_SCRIPTS {
            if let Err(errors) = quantick_pine::compile(source, name) {
                let rendered: Vec<String> = errors.iter().map(|e| e.render(name, source)).collect();
                panic!("embedded {name} must compile:\n{}", rendered.join("\n"));
            }
        }
    }
}
