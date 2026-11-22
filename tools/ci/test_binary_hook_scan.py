#!/usr/bin/env python3
"""Compiled-name checks must work without C-style string boundaries."""

import random
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import binary_hook_scan as scan  # noqa: E402

LAUNCH = 'declare_hooks!["QUANTICK_CONFIG", "QUANTICK_FEEDS"]; "QUANTICK_NOT_DECLARED"'
REGISTRY = """
| `QUANTICK_CONFIG` | operator configuration |
| `QUANTICK_CAPTURE` | capture hook |
| `QUANTICK_LOAD_OLDER` | capture hook also named in schema prose |
"""
CONFIGURATION = b"QUANTICK_CONFIGQUANTICK_FEEDS"


class CompiledNames(unittest.TestCase):
    def test_adjacent_literals_have_no_boundaries(self):
        binary = b"ERRORQUANTICK_CAPTUREQUANTICK_LOAD_OLDEREND"
        names = {"QUANTICK_CAPTURE", "QUANTICK_LOAD_OLDER"}
        self.assertEqual(names, scan.compiled_names(binary, names))

    def test_every_occurrence_is_examined(self):
        binary = b"QUANTICK_UNKNOWN\0QUANTICK_CAPTURE\0QUANTICK_CAPTURE"
        self.assertEqual({"QUANTICK_CAPTURE"}, scan.compiled_names(binary, {"QUANTICK_CAPTURE"}))

    def test_randomised_literal_presence(self):
        names = [f"QUANTICK_{letter}" for letter in "ABCDEFGH"]
        rng = random.Random(20260919)
        for _ in range(200):
            binary = b"".join(rng.choice([b"", b"X", b"\0"]) + rng.choice(names).encode()
                              for _ in range(rng.randrange(12)))
            expected = {name for name in names if name.encode() in binary}
            self.assertEqual(expected, scan.compiled_names(binary, names))


class Verdict(unittest.TestCase):
    def test_default_configuration_only_passes(self):
        configuration, hooks, missing, present = scan.verdict(CONFIGURATION, LAUNCH, REGISTRY)
        self.assertEqual({"QUANTICK_CONFIG", "QUANTICK_FEEDS"}, configuration)
        self.assertEqual({"QUANTICK_CAPTURE", "QUANTICK_LOAD_OLDER"}, hooks)
        self.assertEqual(([], []), (missing, present))

    def test_concatenated_capture_hook_fails(self):
        _, _, _, present = scan.verdict(CONFIGURATION + b"QUANTICK_CAPTUREEND", LAUNCH, REGISTRY)
        self.assertEqual(["QUANTICK_CAPTURE"], present)

    def test_schema_prose_has_no_exemption(self):
        _, _, _, present = scan.verdict(CONFIGURATION + b"QUANTICK_LOAD_OLDER", LAUNCH, REGISTRY)
        self.assertEqual(["QUANTICK_LOAD_OLDER"], present)

    def test_missing_configuration_fails(self):
        _, _, missing, _ = scan.verdict(b"QUANTICK_CONFIG", LAUNCH, REGISTRY)
        self.assertEqual(["QUANTICK_FEEDS"], missing)

    def test_harness_requires_every_registered_hook(self):
        _, _, missing, present = scan.verdict(CONFIGURATION, LAUNCH, REGISTRY, harness=True)
        self.assertEqual(["QUANTICK_CAPTURE", "QUANTICK_LOAD_OLDER"], missing)
        self.assertEqual([], present)
        binary = CONFIGURATION + b"QUANTICK_CAPTUREQUANTICK_LOAD_OLDER"
        self.assertEqual(([], []), scan.verdict(binary, LAUNCH, REGISTRY, harness=True)[2:])

    def test_grouped_store_names_in_registry_prose_are_checked(self):
        registry = REGISTRY + "| `QUANTICK_CAPTURE` and siblings | `QUANTICK_SYMBOLS` |"
        _, hooks, _, present = scan.verdict(CONFIGURATION + b"QUANTICK_SYMBOLS", LAUNCH, registry)
        self.assertIn("QUANTICK_SYMBOLS", hooks)
        self.assertEqual(["QUANTICK_SYMBOLS"], present)

    def test_names_outside_configuration_macro_are_not_allowed(self):
        self.assertNotIn("QUANTICK_NOT_DECLARED", scan.declared_configuration(LAUNCH))

    def test_cli_checks_the_requested_artifact_and_feature_mode(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            binary, launch, registry = (root / name for name in ("app.exe", "launch.rs", "registry.md"))
            binary.write_bytes(CONFIGURATION)
            launch.write_text(LAUNCH)
            registry.write_text(REGISTRY)
            with patch.object(scan, "DEFAULT_LAUNCH", launch), patch.object(scan, "DEFAULT_REGISTRY", registry):
                self.assertEqual(0, scan.main(["--binary", str(binary)]))
                self.assertEqual(1, scan.main(["--binary", str(binary), "--harness"]))
                binary.write_bytes(CONFIGURATION + b"QUANTICK_CAPTUREQUANTICK_LOAD_OLDER")
                self.assertEqual(1, scan.main(["--binary", str(binary)]))
                self.assertEqual(0, scan.main(["--binary", str(binary), "--harness"]))


if __name__ == "__main__":
    unittest.main(verbosity=2)
