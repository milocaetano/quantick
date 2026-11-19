//! The strategy library: the `.pine` files in one folder, listed in a stable
//! order and read by bare file name only.

#[path = "../../stores/src/scratch.rs"]
#[allow(dead_code)]
mod scratch;

use quantick_script_trigger::{Library, LibraryError};

#[test]
fn names_are_the_pine_files_sorted() {
    let dir = scratch::ScratchDir::new("strategy-library-names");
    for name in ["b.pine", "a.pine", "notes.txt"] {
        std::fs::write(dir.path().join(name), "//@version=5\n").expect("writable");
    }
    std::fs::create_dir(dir.path().join("nested.pine")).expect("a folder named like a script");
    let library = Library::new(dir.path());
    assert_eq!(library.names().expect("listable"), ["a.pine", "b.pine"]);
    assert_eq!(library.read("a.pine").expect("readable"), "//@version=5\n");
}

#[test]
fn a_name_that_is_not_a_bare_pine_file_is_refused() {
    let dir = scratch::ScratchDir::new("strategy-library-names-refused");
    let library = Library::new(dir.path());
    for name in ["../escape.pine", "sub/x.pine", "x.txt", ".pine", ""] {
        assert!(
            matches!(library.read(name), Err(LibraryError::BadName(_))),
            "{name:?}"
        );
    }
    assert!(matches!(
        library.read("absent.pine"),
        Err(LibraryError::Missing(name)) if name == "absent.pine"
    ));
}

#[test]
fn a_missing_folder_lists_as_an_error_naming_it() {
    let dir = scratch::ScratchDir::new("strategy-library-missing");
    let gone = dir.path().join("never-made");
    let error = Library::new(&gone).names().expect_err("no folder");
    assert!(error.to_string().contains("never-made"), "{error}");
}
