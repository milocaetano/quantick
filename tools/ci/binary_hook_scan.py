#!/usr/bin/env python3
"""Check compiled hook names, including adjacent Rust string literals.

Default builds must contain configuration and no harness names. With
--harness, every registered hook must be present. Pass --binary to audit a
release executable; the default audits the workspace debug build.
"""

import argparse
import pathlib
import re
import sys

PREFIX = b"QUANTICK_"
DEFAULT_BINARY = pathlib.Path("target/debug/quantick-app" + (".exe" if sys.platform == "win32" else ""))
DEFAULT_LAUNCH = pathlib.Path("crates/app/src/launch.rs")
DEFAULT_REGISTRY = pathlib.Path(".claude/skills/ui-harness/references/hook-registry.md")


def compiled_names(binary, names):
    """Find known literals even when the linker concatenates them.

    Rust strings have lengths, not terminators: uppercase bytes on either
    side cannot prove a name is absent. Search each prefix once, then match
    candidates there; no regex pass over the executable per hook.
    """
    candidates = {name: name.encode("ascii") for name in names}
    found = set()
    start = binary.find(PREFIX)
    while start != -1:
        found.update(name for name, literal in candidates.items()
                     if binary.startswith(literal, start))
        start = binary.find(PREFIX, start + len(PREFIX))
    return found


def declared_configuration(launch_source):
    """The names the composition root declares as operator configuration."""
    declared = launch_source[launch_source.index("declare_hooks!["):]
    return set(re.findall(r"QUANTICK_[A-Z0-9_]+", declared[:declared.index("];")]))


def registry_hooks(registry_source, configuration):
    """Registered harness hooks, excluding operator configuration."""
    # Grouped store overrides occur in a row's prose, not its first cell.
    return set(re.findall(r"QUANTICK_[A-Z0-9_]+", registry_source)) - configuration


def verdict(binary, launch_source, registry_source, harness=False):
    """Return configuration, hooks, missing required names and forbidden names."""
    configuration = declared_configuration(launch_source)
    hooks = registry_hooks(registry_source, configuration)
    names = compiled_names(binary, configuration | hooks)
    required = configuration | hooks if harness else configuration
    return configuration, hooks, sorted(required - names), sorted(hooks & names) if not harness else []


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, default=DEFAULT_BINARY)
    parser.add_argument("--harness", action="store_true", help="require all harness families")
    args = parser.parse_args(argv)
    configuration, hooks, missing, present = verdict(
        args.binary.read_bytes(),
        DEFAULT_LAUNCH.read_text(encoding="utf-8"),
        DEFAULT_REGISTRY.read_text(encoding="utf-8"),
        args.harness,
    )
    if not configuration or not hooks or missing:
        print(f"empty declarations or required names missing: {missing}", file=sys.stderr)
        return 1
    if present:
        print(f"harness hooks in the default binary: {present}", file=sys.stderr)
        return 1
    print(f"{args.binary}: {len(configuration)} configuration names present, "
          f"{len(hooks)} harness hooks {'present' if args.harness else 'absent'}")
    print("Configuration: " + ", ".join(sorted(configuration)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
