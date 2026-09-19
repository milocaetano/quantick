#!/usr/bin/env python3
"""Offline tests for the per-PR read-cost calculator."""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "quantick_read_cost", os.path.join(HERE, "measure.py")
)
read_cost = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = read_cost
SPEC.loader.exec_module(read_cost)


def run(root, *args):
    return subprocess.run(
        args,
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ).stdout.strip()


def write(root, rel, text):
    path = os.path.join(root, *rel.split("/"))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as stream:
        stream.write(text)


def commit(root, message):
    run(root, "git", "add", "-A")
    run(
        root,
        "git",
        "-c",
        "user.name=Read Cost Fixture",
        "-c",
        "user.email=read-cost@example.invalid",
        "commit",
        "-m",
        message,
    )
    return run(root, "git", "rev-parse", "HEAD")


class Repository:
    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        run(self.root, "git", "init", "--quiet")

    def close(self):
        self.tmp.cleanup()

    def base_crate(self, name="core"):
        write(
            self.root,
            f"crates/{name}/Cargo.toml",
            f'[package]\nname = "{name}"\nversion = "0.1.0"\n',
        )


def indexed(report):
    return {entry["path"]: entry for entry in report["files"]}


class ReadCostTest(unittest.TestCase):
    def setUp(self):
        self.repo = Repository()

    def tearDown(self):
        self.repo.close()

    def test_direct_grouped_reexport_and_parent_references_are_deduplicated(self):
        self.repo.base_crate()
        write(
            self.repo.root,
            "crates/core/src/lib.rs",
            "mod changed;\nmod deep;\nmod large;\nmod shared;\npub struct Root;\n",
        )
        write(
            self.repo.root,
            "crates/core/src/changed.rs",
            "use crate::{shared::One, shared::{self, Two}};\n"
            "pub use crate::shared::One as PublicOne;\n"
            "use super::Root;\n"
            "use external_crate::Thing;\n"
            "pub fn changed() {}\n"
            "#[cfg(test)]\n"
            "mod tests { use crate::large::Large; }\n",
        )
        write(
            self.repo.root,
            "crates/core/src/shared.rs",
            "use crate::deep::Deep;\npub struct One;\npub struct Two;\n",
        )
        write(self.repo.root, "crates/core/src/deep.rs", "pub struct Deep;\n")
        write(self.repo.root, "crates/core/src/large.rs", "pub struct Large;\n")
        base = commit(self.repo.root, "base")
        with open(
            os.path.join(self.repo.root, "crates", "core", "src", "changed.rs"),
            "a",
            encoding="utf-8",
            newline="\n",
        ) as stream:
            stream.write("pub fn added() {}\n")
        head = commit(self.repo.root, "change one module")

        report = read_cost.measure(self.repo.root, base, head)
        files = indexed(report)

        self.assertEqual(report["scope"], "direct_in_crate_modules")
        self.assertEqual(set(files), {
            "crates/core/src/changed.rs",
            "crates/core/src/lib.rs",
            "crates/core/src/shared.rs",
        })
        self.assertEqual(files["crates/core/src/changed.rs"]["roles"], ["touched"])
        self.assertEqual(files["crates/core/src/lib.rs"]["roles"], ["referenced"])
        self.assertEqual(files["crates/core/src/shared.rs"]["roles"], ["referenced"])
        # changed: 6, root: 5, shared: 3. Test-only and transitive references
        # do not pull in large.rs or deep.rs.
        self.assertEqual(report["production_lines"], 14)
        self.assertEqual(report["unresolved"], [])

    def test_nested_sibling_and_external_module_declarations_resolve(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod outer;\n")
        write(
            self.repo.root,
            "crates/core/src/outer.rs",
            "mod leaf;\nmod sibling;\npub struct Parent;\n"
            "pub fn call_sibling() { sibling::sibling(); }\n",
        )
        write(
            self.repo.root,
            "crates/core/src/outer/leaf.rs",
            "use super::{Parent, sibling::Sibling};\npub fn leaf() {}\n",
        )
        write(
            self.repo.root,
            "crates/core/src/outer/sibling.rs",
            "pub struct Sibling; pub fn sibling() {}\n",
        )
        base = commit(self.repo.root, "nested base")
        write(
            self.repo.root,
            "crates/core/src/outer.rs",
            "mod leaf;\nmod sibling;\npub struct Parent;\n"
            "pub fn call_sibling() { sibling::sibling(); }\n"
            "pub fn changed_parent() {}\n",
        )
        write(
            self.repo.root,
            "crates/core/src/outer/leaf.rs",
            "use super::{Parent, sibling::Sibling};\npub fn leaf() { let _ = 1; }\n",
        )
        head = commit(self.repo.root, "change nested leaf")

        report = read_cost.measure(self.repo.root, base, head)

        self.assertEqual(set(indexed(report)), {
            "crates/core/src/outer.rs",
            "crates/core/src/outer/leaf.rs",
            "crates/core/src/outer/sibling.rs",
        })
        self.assertEqual(report["production_lines"], 8)
        self.assertTrue(
            any(
                edge["expression"] == "sibling::sibling"
                and edge["to"] == "crates/core/src/outer/sibling.rs"
                for edge in report["references"]
            )
        )

    def test_mod_rs_layout_resolves_its_child_without_crossing_the_crate(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod folder;\n")
        write(
            self.repo.root,
            "crates/core/src/folder/mod.rs",
            "mod child;\npub fn folder() {}\n",
        )
        write(
            self.repo.root,
            "crates/core/src/folder/child.rs",
            "pub fn child() {}\n",
        )
        base = commit(self.repo.root, "mod rs base")
        write(
            self.repo.root,
            "crates/core/src/folder/mod.rs",
            "mod child;\npub fn folder() { let _ = 1; }\n",
        )
        head = commit(self.repo.root, "touch mod rs")

        report = read_cost.measure(self.repo.root, base, head)

        self.assertEqual(set(indexed(report)), {
            "crates/core/src/folder/child.rs",
            "crates/core/src/folder/mod.rs",
        })
        self.assertEqual(report["production_lines"], 3)

    def test_cross_crate_and_docs_only_changes_do_not_invent_read_cost(self):
        self.repo.base_crate("one")
        self.repo.base_crate("two")
        write(
            self.repo.root,
            "crates/one/src/lib.rs",
            "use two::remote::Remote;\npub fn one() {}\n",
        )
        write(self.repo.root, "crates/two/src/lib.rs", "pub mod remote;\n")
        write(self.repo.root, "crates/two/src/remote.rs", "pub struct Remote;\n")
        write(self.repo.root, "README.md", "before\n")
        base = commit(self.repo.root, "two crates")
        write(self.repo.root, "README.md", "after\n")
        head = commit(self.repo.root, "docs only")

        report = read_cost.measure(self.repo.root, base, head)

        self.assertEqual(report["production_lines"], 0)
        self.assertEqual(report["files"], [])
        self.assertEqual(report["production_change"], "not_applicable")

    def test_deleted_file_uses_the_base_preimage_and_its_direct_references(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod gone;\nmod support;\n")
        write(
            self.repo.root,
            "crates/core/src/gone.rs",
            "use crate::support::Support;\npub fn gone() {}\n",
        )
        write(self.repo.root, "crates/core/src/support.rs", "pub struct Support;\n")
        base = commit(self.repo.root, "deletion base")
        os.remove(os.path.join(self.repo.root, "crates", "core", "src", "gone.rs"))
        head = commit(self.repo.root, "delete source")

        report = read_cost.measure(self.repo.root, base, head)
        files = indexed(report)

        self.assertEqual(set(files), {
            "crates/core/src/gone.rs",
            "crates/core/src/support.rs",
        })
        self.assertEqual(files["crates/core/src/gone.rs"]["revision"], "merge_base")
        self.assertEqual(files["crates/core/src/support.rs"]["revision"], "head")
        self.assertEqual(report["production_lines"], 3)

    def test_diverged_base_uses_the_actual_diff_preimage_for_a_deletion(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod gone;\nmod support;\n")
        write(
            self.repo.root,
            "crates/core/src/gone.rs",
            "use crate::support::Support;\npub fn gone() {}\n",
        )
        write(self.repo.root, "crates/core/src/support.rs", "pub struct Support;\n")
        common = commit(self.repo.root, "common ancestor")

        run(self.repo.root, "git", "checkout", "-b", "head-side", common)
        os.remove(os.path.join(self.repo.root, "crates", "core", "src", "gone.rs"))
        head = commit(self.repo.root, "delete on head")

        run(self.repo.root, "git", "checkout", "-b", "base-side", common)
        write(
            self.repo.root,
            "crates/core/src/gone.rs",
            "use crate::support::Support;\npub fn gone() {}\npub fn base_only() {}\n",
        )
        base = commit(self.repo.root, "advance base differently")

        report = read_cost.measure(self.repo.root, base, head)
        files = indexed(report)

        self.assertEqual(report["base"], base)
        self.assertEqual(report["merge_base"], common)
        self.assertEqual(files["crates/core/src/gone.rs"]["revision"], "merge_base")
        self.assertEqual(files["crates/core/src/gone.rs"]["lines"], 2)
        self.assertEqual(report["production_lines"], 3)

    def test_rename_counts_the_head_path_once_and_retains_both_names(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod old;\nmod support;\n")
        write(
            self.repo.root,
            "crates/core/src/old.rs",
            "use crate::support::Support;\npub fn item() {}\n",
        )
        write(self.repo.root, "crates/core/src/support.rs", "pub struct Support;\n")
        base = commit(self.repo.root, "rename base")
        run(
            self.repo.root,
            "git",
            "mv",
            "crates/core/src/old.rs",
            "crates/core/src/new.rs",
        )
        head = commit(self.repo.root, "rename source")

        report = read_cost.measure(self.repo.root, base, head)
        files = indexed(report)

        self.assertEqual(set(files), {
            "crates/core/src/new.rs",
            "crates/core/src/support.rs",
        })
        self.assertEqual(files["crates/core/src/new.rs"]["roles"], ["touched"])
        self.assertEqual(report["changes"][0]["old_path"], "crates/core/src/old.rs")
        self.assertEqual(report["changes"][0]["new_path"], "crates/core/src/new.rs")
        self.assertTrue(report["changes"][0]["status"].startswith("R"))

    def test_path_attributes_and_inline_module_scopes_are_explicitly_unresolved(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "mod changed;\nmod sibling;\n")
        write(self.repo.root, "crates/core/src/sibling.rs", "pub struct Sibling;\n")
        write(self.repo.root, "crates/core/src/alternate.rs", "pub struct Alternate;\n")
        write(
            self.repo.root,
            "crates/core/src/changed.rs",
            '#[path = "alternate.rs"]\nmod alternate;\n'
            "mod local { use super::super::sibling::Sibling; }\n"
            "pub fn changed() {}\n",
        )
        base = commit(self.repo.root, "unsupported base")
        with open(
            os.path.join(self.repo.root, "crates", "core", "src", "changed.rs"),
            "a",
            encoding="utf-8",
            newline="\n",
        ) as stream:
            stream.write("pub fn added() {}\n")
        head = commit(self.repo.root, "touch unsupported source")

        report = read_cost.measure(self.repo.root, base, head)
        reasons = {entry["reason"] for entry in report["unresolved"]}

        self.assertIn("path_attribute", reasons)
        self.assertIn("inline_module_scope", reasons)
        self.assertNotIn("crates/core/src/alternate.rs", indexed(report))
        self.assertIn("inline module", " ".join(report["limitations"]))

    def test_explicit_shas_and_json_bytes_are_reproducible(self):
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "pub fn before() {}\n")
        base = commit(self.repo.root, "base")
        write(
            self.repo.root,
            "crates/core/src/lib.rs",
            "pub fn before() {}\npub fn after() {}\n",
        )
        head = commit(self.repo.root, "head")

        first = read_cost.render(read_cost.measure(self.repo.root, base[:12], head[:12]))
        second = read_cost.render(read_cost.measure(self.repo.root, base, head))
        decoded = json.loads(first)

        self.assertEqual(first, second)
        self.assertEqual(decoded["base"], base)
        self.assertEqual(decoded["head"], head)
        self.assertEqual(decoded["production_lines"], 2)
        self.assertTrue(first.endswith("\n"))


if __name__ == "__main__":
    unittest.main()
