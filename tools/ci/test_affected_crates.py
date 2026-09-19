#!/usr/bin/env python3
"""Tests for affected_crates.py: python tools/ci/test_affected_crates.py

The synthetic cases pin the selection rules on a small hand-drawn graph. The
workspace cases run against this repository's real `cargo metadata` and check
the reverse-dependency closure against `cargo tree --invert`, a second and
independent reading of the same graph, so neither side can drift alone.
"""

import importlib.util
import json
import os
import shutil
import subprocess
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location("affected_crates", os.path.join(HERE, "affected_crates.py"))
affected_crates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(affected_crates)

# engine <- indicators <- app; engine <- sim (dev-dependency) ; guards alone.
PACKAGES = {
    "quantick-engine": {"dir": "crates/engine", "deps": set()},
    "quantick-indicators": {"dir": "crates/indicators", "deps": {"quantick-engine"}},
    "quantick-app": {"dir": "crates/app", "deps": {"quantick-indicators"}},
    "quantick-sim": {"dir": "crates/sim", "deps": {"quantick-engine"}},
    "quantick-feed": {"dir": "crates/feed", "deps": set()},
    "quantick-feed-mt5": {"dir": "crates/feed-mt5", "deps": {"quantick-feed"}},
    "quantick-guards": {"dir": "crates/guards", "deps": set()},
}

# Which tracked crate files name which path, as `git grep -F` would report.
REFERENCES = {
    "engine/tests/fixtures/tick.csv": ["crates/app/src/parity_tests.rs"],
    "docs/quality/envelope.md": ["crates/app/src/envelope_tests.rs"],
    "bridge/mt5/tests": ["crates/feed-mt5/tests/bridge_paging.rs"],
}


SEARCHES = []


def referrers(wanted):
    SEARCHES.append(list(wanted))
    return [(path, needle) for needle in wanted for path in REFERENCES.get(needle, [])]


def names(changed):
    return sorted(affected_crates.affected(changed, PACKAGES, referrers))


class Selection(unittest.TestCase):
    def test_a_docs_only_diff_runs_only_the_guards(self):
        self.assertEqual(names(["docs/workflow/delivery.md", ".claude/hooks/README.md"]), ["quantick-guards"])

    def test_an_engine_change_runs_every_dependent_including_dev_dependents(self):
        self.assertEqual(
            names(["crates/engine/src/lib.rs"]),
            ["quantick-app", "quantick-engine", "quantick-guards", "quantick-indicators", "quantick-sim"],
        )

    def test_a_leaf_change_runs_only_the_leaf(self):
        self.assertEqual(names(["crates/app/src/main.rs"]), ["quantick-app", "quantick-guards"])

    def test_a_fixture_selects_the_crate_that_reads_it_by_path(self):
        # app reads engine's fixture; engine's dependents come with engine.
        selected = affected_crates.affected(["crates/engine/tests/fixtures/tick.csv"], PACKAGES, referrers)
        self.assertEqual(selected["quantick-app"], "crates/app/src/parity_tests.rs names engine/tests/fixtures/tick.csv")

    def test_a_file_outside_every_crate_selects_its_readers(self):
        self.assertEqual(names(["docs/quality/envelope.md"]), ["quantick-app", "quantick-guards"])

    def test_a_directory_a_test_walks_selects_that_test(self):
        self.assertEqual(names(["bridge/mt5/tests/test_paging.py"]), ["quantick-feed-mt5", "quantick-guards"])

    def test_a_workspace_input_runs_everything(self):
        for path in ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml", ".cargo/config.toml"):
            with self.subTest(path=path):
                self.assertEqual(names([path, "docs/quality/envelope.md"]), sorted(PACKAGES))

    def test_a_large_diff_searches_the_tree_once(self):
        SEARCHES.clear()
        names([f"docs/sweep/file_{i}.md" for i in range(1000)] + ["docs/quality/envelope.md"])
        self.assertEqual(len(SEARCHES), 1)

    def test_a_workspace_input_skips_the_search(self):
        SEARCHES.clear()
        names(["Cargo.lock", "docs/quality/envelope.md"])
        self.assertEqual(SEARCHES, [])

    def test_a_crate_manifest_is_not_the_workspace_manifest(self):
        self.assertEqual(names(["crates/feed/Cargo.toml"]), ["quantick-feed", "quantick-feed-mt5", "quantick-guards"])

    def test_the_deepest_manifest_directory_owns_a_file(self):
        packages = dict(PACKAGES, **{"quantick-inner": {"dir": "crates/feed/inner", "deps": set()}})
        self.assertEqual(affected_crates.owner("crates/feed/inner/src/lib.rs", packages), "quantick-inner")
        self.assertEqual(affected_crates.owner("crates/feed-mt5/src/lib.rs", packages), "quantick-feed-mt5")

    def test_rust_files_are_searched_by_exact_path_only(self):
        self.assertEqual(
            affected_crates.needles("crates/control/tests/support/schema.rs"),
            ["crates/control/tests/support/schema.rs", "control/tests/support/schema.rs"],
        )


@unittest.skipUnless(shutil.which("cargo"), "cargo is not installed")
class Workspace(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = os.path.dirname(os.path.dirname(HERE))
        metadata = subprocess.run(
            ("cargo", "metadata", "--format-version", "1", "--no-deps"),
            cwd=root, check=True, capture_output=True, text=True,
        ).stdout
        cls.root = root
        _, cls.packages = affected_crates.workspace(json.loads(metadata))

    def inverted_tree(self, package):
        """Every workspace crate `cargo tree` says depends on `package`."""
        out = subprocess.run(
            ("cargo", "tree", "--invert", package, "--workspace", "--edges", "normal,dev,build",
             "--target", "all", "--prefix", "none", "--format", "{p}"),
            cwd=self.root, check=True, capture_output=True, text=True,
        ).stdout
        return {line.split()[0] for line in out.splitlines() if line.split() and line.split()[0] in self.packages}

    def test_an_engine_change_runs_every_crate_cargo_tree_says_depends_on_engine(self):
        selected = affected_crates.affected(["crates/engine/src/lib.rs"], self.packages, lambda wanted: [])
        expected = self.inverted_tree("quantick-engine") | set(affected_crates.ALWAYS)
        self.assertEqual(set(selected), expected)
        # Not a vacuous equality: engine has dependents, and they are not all.
        self.assertGreater(len(expected), 2)
        self.assertLess(len(expected), len(self.packages))

    def test_every_crate_closure_matches_cargo_tree(self):
        for package in sorted(self.packages):
            with self.subTest(package=package):
                directory = self.packages[package]["dir"]
                selected = affected_crates.affected([f"{directory}/src/lib.rs"], self.packages, lambda wanted: [])
                self.assertEqual(set(selected), self.inverted_tree(package) | set(affected_crates.ALWAYS))


if __name__ == "__main__":
    unittest.main()
