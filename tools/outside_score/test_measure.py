#!/usr/bin/env python3
"""Tests for measure.py: python tools/outside_score/test_measure.py

Each case writes a small tree whose expected rows are counted by hand from the
fixture text, never read back from the measurement.
"""

import importlib.util
import io
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location("measure", os.path.join(HERE, "measure.py"))
measure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(measure)

# 1 fn line + 3 body lines + 1 closing brace = a 5-line function. Its body
# holds a brace, a fn keyword and a hook name inside literals, which must not
# change its length, and a lifetime and char literals the lexer must not
# mistake for each other.
LIB = r'''//! A crate. The word fn in a comment is not a function. {
use std::fmt;

pub(crate) fn tricky<'a>(s: &'a str) -> usize {
    let raw = r#"fn fake() { "quoted" }"#; let c = '{'; let q = '\'';
    let b = b'}'; let text = "unbalanced { brace and fn inside";
    s.len() + raw.len() + text.len() + (c as usize) + (q as usize) + (b as usize)
}

/* block /* nested { */ comment } */
pub(super) fn short() -> u8 {
    let v: Option<u8> = None;
    v.unwrap()
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Square;

impl Square {
    pub fn side(&self) -> f64 { 1.0 }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "sq") }
}

pub fn evens() -> impl Iterator<Item = u8> {
    (0..4).filter(|n| n % 2 == 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_is_not_production() {
        let _ = "QUANTICK_TEST_ONLY";
        panic!("never counted");
    }
}
'''

# A second file of the UI crate that names the toolkit, and a second inherent
# impl of Square in another file: the spread is two files.
UI_VIEW = '''use egui::Ui;

pub fn draw(ui: &mut Ui) {
    let _ = std::env::var("QUANTICK_DEMO_HOOK");
    ui.label("x");
}

impl crate::Square {
    pub fn paint(&self) {}
}
'''

UI_FREE = '''pub fn hook() -> Option<String> {
    std::env::var("QUANTICK_DEMO_HOOK").ok()
        .or_else(|| std::env::var("QUANTICK_OTHER").ok())
}
'''

SIDECAR = '''#[test]
fn sidecar_lines_are_test_lines() {
    assert_eq!(1, 1);
}
'''


def write(root, rel, text):
    path = os.path.join(root, *rel.split("/"))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def rows(text):
    return dict(line.split("\t", 1) for line in text.splitlines())


class MeasureTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        write(root, "crates/app/src/lib.rs", LIB)
        write(root, "crates/app/src/view.rs", UI_VIEW)
        write(root, "crates/app/src/hooks.rs", UI_FREE)
        write(root, "crates/app/src/view_tests.rs", SIDECAR)
        write(root, "crates/app/tests/integration.rs", SIDECAR)
        self.out = measure.render(root, "app", top=20)
        self.rows = rows(self.out)

    def tearDown(self):
        self.tmp.cleanup()

    def test_literals_and_comments_do_not_change_function_length(self):
        self.assertEqual(self.rows["fn.longest.crates/app/src/lib.rs::tricky"], "5")
        self.assertEqual(self.rows["fn.longest.crates/app/src/lib.rs::short"], "4")

    def test_trait_declarations_are_not_functions_and_test_items_are_skipped(self):
        names = [k for k in self.rows if k.startswith("fn.longest.")]
        self.assertFalse(any(k.endswith("::area") for k in names))
        self.assertFalse(any("a_test_is_not_production" in k for k in names))
        self.assertFalse(any("sidecar" in k for k in names))
        # tricky, short, side, fmt, evens, draw, paint, hook
        self.assertEqual(self.rows["fns.production"], "8")

    def test_panic_sites_count_code_only(self):
        # `v.unwrap()` in `short`; the panic! lives in the test module.
        self.assertEqual(self.rows["panic_sites"], "1")

    def test_inherent_impls_spread_across_files_and_trait_impls_do_not(self):
        self.assertEqual(self.rows["crate.app.impl_spread.Square"], "2")
        self.assertFalse(any(".impl_spread.Iterator" in k for k in self.rows))

    def test_crate_visibility_counts_pub_crate_and_pub_super(self):
        prod = int(self.rows["crate.app.lines.production"])
        expected = f"{1000 * 2 / prod:.2f}"
        self.assertEqual(self.rows["crate.app.crate_visibility_per_kloc"], expected)

    def test_ui_free_share_and_harness_hooks(self):
        # hooks.rs never names the toolkit; lib.rs neither. view.rs does.
        prod = int(self.rows["ui_crate.lines.production"])
        free = int(self.rows["ui_crate.ui_free_lines"])
        view_lines = 8  # every non-blank line of UI_VIEW
        self.assertEqual(prod - free, view_lines)
        # QUANTICK_DEMO_HOOK twice and QUANTICK_OTHER once; the test-only
        # name inside #[cfg(test)] is excluded.
        self.assertEqual(self.rows["ui_crate.harness_hooks"], "2")

    def test_test_files_and_test_modules_count_as_test_lines(self):
        # 4 sidecar lines, 4 integration lines, and the cfg(test) module's 8,
        # its attribute included.
        self.assertEqual(self.rows["lines.test"], "16")

    def test_long_functions_are_also_reported_per_100k_lines(self):
        with tempfile.TemporaryDirectory() as root:
            # 1 signature line + 205 body lines + 1 closing brace = 207.
            body = "    let _x = 1;\n" * 205
            write(root, "crates/core/src/lib.rs", "pub fn big() {\n" + body + "}\n")
            out = rows(measure.render(root, "app", top=20))
        self.assertEqual(out["fn.longest.crates/core/src/lib.rs::big"], "207")
        self.assertEqual(out["fns.over_200"], "1")
        self.assertEqual(out["fns.over_200.per_100k"], f"{100000 / 207:.1f}")

    def test_a_nested_crate_is_named_by_its_own_manifest(self):
        root = self.tmp.name
        write(root, "crates/viewer/re_view/Cargo.toml", '[package]\nname = "re_view"\n')
        write(root, "crates/viewer/re_view/src/lib.rs", "pub fn one() -> u8 {\n    1\n}\n")
        out = rows(measure.render(root, "app", top=20))
        self.assertEqual(out["crate.re_view.lines.production"], "3")
        self.assertNotIn("crate.viewer.lines.production", out)

    def test_inner_and_any_cfg_test_are_test_code(self):
        root = self.tmp.name
        # A test-only file declared by an inner attribute: 4 lines, all test.
        write(root, "crates/app/src/support.rs", "#![cfg(test)]\npub fn helper() {\n    let _ = 1;\n}\n")
        # Test support also published by a feature: 4 lines, all test.
        write(
            root,
            "crates/app/src/fake.rs",
            '#[cfg(any(test, feature = "fake"))]\npub fn fake() -> u8 {\n    Some(1).unwrap()\n}\n',
        )
        out = rows(measure.render(root, "app", top=20))
        self.assertEqual(out["lines.test"], str(16 + 4 + 4))
        self.assertEqual(out["panic_sites"], "1")  # still only `short`'s unwrap
        self.assertFalse(any(k.endswith("::helper") or k.endswith("::fake") for k in out))

    def test_an_impl_on_a_trait_object_is_named_by_the_trait(self):
        with tempfile.TemporaryDirectory() as root:
            write(root, "crates/core/src/a.rs", "impl dyn Shape {\n    fn a(&self) {}\n}\n")
            write(root, "crates/core/src/b.rs", "impl dyn Shape {\n    fn b(&self) {}\n}\n")
            out = rows(measure.render(root, "app", top=20))
        self.assertEqual(out["crate.core.impl_spread.Shape"], "2")

    def test_harness_hooks_count_every_crate_and_the_ui_crate_apart(self):
        with tempfile.TemporaryDirectory() as root:
            write(root, "crates/app/src/lib.rs", 'pub fn a() { let _ = "QUANTICK_A"; }\n')
            write(root, "crates/feed/src/lib.rs", 'pub fn b() { let _ = ("QUANTICK_A", "QUANTICK_B"); }\n')
            out = rows(measure.render(root, "app", top=20))
        self.assertEqual(out["ui_crate.harness_hooks"], "1")
        self.assertEqual(out["harness_hooks"], "2")

    def test_a_flag_without_its_value_prints_usage(self):
        for argv in (["m", self.tmp.name, "--top"], ["m", self.tmp.name, "--top", "x"], ["m", "--ui-crate"]):
            stderr = sys.stderr
            sys.stderr = io.StringIO()
            try:
                code = measure.main(argv)
                text = sys.stderr.getvalue()
            finally:
                sys.stderr = stderr
            self.assertEqual(code, 2, argv)
            self.assertIn("Usage:", text)

    def test_output_is_deterministic(self):
        self.assertEqual(self.out, measure.render(self.tmp.name, "app", top=20))


if __name__ == "__main__":
    unittest.main()
