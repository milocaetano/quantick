//! The shared scratch module's own tests, mounted by `stores` alone so the
//! crates that mount `scratch.rs` with `#[path]` do not each rerun them.

use super::scratch::*;
use std::fs;

#[test]
fn two_scratch_directories_never_share_a_path() {
    let first = ScratchDir::new("same-label");
    let second = ScratchDir::new("same-label");
    assert_ne!(first.path(), second.path());
}

#[test]
fn a_scratch_directory_is_gone_once_it_is_dropped() {
    let path = {
        let scratch = ScratchDir::new("removed-on-drop");
        scratch.write("nested/file.toml", "content");
        assert!(scratch.path().exists(), "the directory was created");
        scratch.path().to_path_buf()
    };
    assert!(!path.exists(), "the tree went with the value: {path:?}");
}

#[test]
fn a_scratch_path_opens_with_the_mounting_package_name() {
    let scratch = ScratchDir::new("prefix");
    let name = scratch.path().file_name().expect("a directory name");
    let name = name.to_string_lossy();
    assert!(
        name.starts_with(&format!("{}-{}-", env!("CARGO_PKG_NAME"), run_token())),
        "the folder names its crate and run: {name}"
    );
}

#[test]
fn the_run_token_carries_the_pid_and_is_stable_within_a_process() {
    let token = run_token();
    assert!(
        token.starts_with(&format!("{}-", std::process::id())),
        "the token opens with this process's id: {token}"
    );
    assert_eq!(token, run_token(), "the token is read once, not per call");
}

#[test]
fn a_scratch_file_goes_with_its_directory() {
    let path = {
        let file = ScratchFile::new("file-drop", "state.toml");
        fs::write(&file, "x").expect("the scratch file is writable");
        assert!(file.path().exists(), "the file was written");
        file.path().to_path_buf()
    };
    assert!(!path.exists(), "the file went with its directory: {path:?}");
}

#[test]
fn a_thread_directory_answers_the_same_path_twice() {
    let first = thread_dir("stable-label");
    let second = thread_dir("stable-label");
    assert_eq!(first, second);
    assert!(first.exists(), "it was created");
}

#[test]
fn another_thread_gets_its_own_directory_for_the_same_label() {
    let here = thread_dir("shared-label");
    let there = std::thread::spawn(|| thread_dir("shared-label"))
        .join()
        .expect("the thread finished");
    assert_ne!(
        here, there,
        "two threads must not resolve one directory for one label"
    );
}

#[test]
fn a_thread_directory_goes_when_its_thread_ends() {
    let path = std::thread::spawn(|| thread_dir("ends-with-the-thread"))
        .join()
        .expect("the thread finished");
    assert!(
        !path.exists(),
        "the thread's own directory went with it: {path:?}"
    );
}
