#!/usr/bin/env python3
"""Assert that the default binary names no harness hook.

The shipped bytes, not the source. A default `cargo build --workspace` has
just produced `target/debug/quantick-app` and nothing has rebuilt it with a
feature, so every `QUANTICK_*` name the harness registry lists must be absent
from it, and the composition root's own declared configuration must be present
— the control that proves the scan can see a name at all.

This used to be a heredoc inside `ci.yml` that called `re.search` once per
registry name. That is one pass over a half-gigabyte binary per name, and the
lookbehind in the pattern defeated the literal-prefix optimisation, so each
pass walked the file a byte at a time: 122 names, about nine seconds each,
eighteen minutes for a step whose whole job is a set comparison. It now reads
the binary once, collecting every `QUANTICK_*` run that begins at a boundary,
and compares sets. The verdict is unchanged — see `boundary_names` for why the
one-pass form and the per-name lookbehind accept exactly the same names.
"""

import pathlib
import re
import sys

PREFIX = b"QUANTICK_"
# The character class the old per-name pattern used on both sides.
NAME_BYTES = frozenset(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")

# Prose, not a read: a published control-schema description names the hook a
# capture drives the button with, so the string is in the default binary
# without the hook being readable there.
PROSE_ONLY = frozenset({"QUANTICK_LOAD_OLDER"})

DEFAULT_BINARY = pathlib.Path("target/debug/quantick-app")
DEFAULT_LAUNCH = pathlib.Path("crates/app/src/launch.rs")
DEFAULT_REGISTRY = pathlib.Path(
    ".claude/skills/ui-harness/references/hook-registry.md"
)


def boundary_names(binary):
    """Every `QUANTICK_*` name in `binary` that stands on its own.

    The old form asked, per name N, whether some occurrence of N was preceded
    and followed by no name character. Equivalently: whether N is a maximal
    run of name characters that starts at a boundary. A run is maximal by
    construction here, because the scan consumes name characters to the end,
    so the trailing condition holds for free; the leading condition is the one
    byte tested below. Any other occurrence of N inside a longer run is
    preceded by a name character, which is exactly what the old lookbehind
    rejected, so neither form counts it.

    `bytes.find` is a memory-speed substring search, and the names are few, so
    this is one pass over the file rather than one pass per name.
    """
    found = set()
    start = binary.find(PREFIX)
    while start != -1:
        end = start + len(PREFIX)
        while end < len(binary) and binary[end] in NAME_BYTES:
            end += 1
        if start == 0 or binary[start - 1] not in NAME_BYTES:
            found.add(binary[start:end].decode("ascii"))
        start = binary.find(PREFIX, end)
    return found


def declared_configuration(launch_source):
    """The `QUANTICK_*` names the composition root declares as configuration."""
    declared = launch_source[launch_source.index("declare_hooks![") :]
    return set(re.findall(r"QUANTICK_[A-Z0-9_]+", declared[: declared.index("];")]))


def registry_hooks(registry_source, configuration):
    """The harness hooks the registry lists, minus the configuration names."""
    hooks = {
        name
        for line in registry_source.splitlines()
        if line.startswith("| `QUANTICK_")
        for name in re.findall(r"QUANTICK_[A-Z0-9_]+", line.split(" | ")[0])
    }
    return hooks - configuration


def verdict(binary, launch_source, registry_source):
    """Return `(configuration, hooks, missing, present)` for these inputs."""
    configuration = declared_configuration(launch_source)
    hooks = registry_hooks(registry_source, configuration)
    names = boundary_names(binary)
    missing = sorted(name for name in configuration if name.encode() not in binary)
    present = sorted((hooks - PROSE_ONLY) & names)
    return configuration, hooks, missing, present


def main():
    configuration, hooks, missing, present = verdict(
        DEFAULT_BINARY.read_bytes(),
        DEFAULT_LAUNCH.read_text(encoding="utf-8"),
        DEFAULT_REGISTRY.read_text(encoding="utf-8"),
    )
    if not configuration or missing:
        print(f"configuration missing from the binary: {missing}", file=sys.stderr)
        return 1
    if present:
        print(f"harness hooks in the default binary: {present}", file=sys.stderr)
        return 1
    print(
        f"{len(configuration)} configuration names present, "
        f"{len(hooks)} harness hooks absent"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
