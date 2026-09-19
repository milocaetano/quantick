//! The script library: where `.pine` sources come from.
//!
//! The library itself — the embedded starter scripts and the folder scan —
//! lives in `quantick-stores`. The window resolves the folder: from
//! `QUANTICK_INDICATORS_DIR` (read by the launch root), falling back to
//! `./indicators` beside the working directory.

use std::path::PathBuf;

#[cfg(test)]
pub(crate) use quantick_stores::script_library::EMBEDDED_SCRIPTS;
pub(crate) use quantick_stores::script_library::ScriptLibrary;

/// Default scripts folder, relative to the working directory.
const DEFAULT_DIR: &str = "indicators";

/// The library over the folder this launch resolves.
pub(crate) fn scan() -> ScriptLibrary {
    let dir = crate::launch::operator_paths()
        .indicators_dir
        .as_deref()
        .map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from);
    ScriptLibrary::open(&dir)
}

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
