#!/bin/sh
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
python3 tools/test_regen.py
cargo test -p quantick-control-handlers generation::tests
cargo test -p quantick-control-host catalog_metadata
cargo test -p quantick-control --test published_schema_compatibility
cargo test -p quantick-control --test canonical_contract
sh tools/regen.sh --check control
sh tools/regen.sh --check control
sh tools/regen.sh --check ui
python3 tools/test_regen.py --real
