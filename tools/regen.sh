#!/bin/sh
# All ordinary generators. Control and UI targets never compile the app.
set -eu
usage() {
    echo 'Usage: sh tools/regen.sh --check|--write [all|control|ui|hooks|pine-oracle]'
    echo 'all: control schemas/catalog/inventory/retry matrix, UI matrix, hook registry'
    echo 'pine-oracle: explicit external fixture refresh (--write only; npm dependencies required)'
}
mode=${1:---check}
target=${2:-all}
case "$mode" in --help) usage; exit 0;; --check|--write) ;; *) usage >&2; exit 2;; esac
[ "$#" -le 2 ] || { usage >&2; exit 2; }
case "$target" in all|control|ui|hooks|pine-oracle) ;; *) usage >&2; exit 2;; esac
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
if [ "$target" = pine-oracle ]; then
    [ "$mode" = --write ] || { echo 'pine-oracle is an opt-in refresh; use --write' >&2; exit 2; }
    exec npm --prefix tools/pine-oracle run regenerate
fi

stage=$(mktemp -d)
pending=
trap 'test -z "$pending" || rm -f "$pending"; rm -f "$stage.paths"; rm -rf "$stage"' EXIT HUP INT TERM
case "$target" in all|control)
    cargo run -q -p quantick-control-handlers --bin regen-control -- --write --root "$stage"
esac
case "$target" in all|ui)
    mkdir -p "$stage/docs/control-plane"
    cargo run -q -p quantick-operability --example export_ui_behaviour_matrix >"$stage/docs/control-plane/ui-behaviour-matrix.md"
esac
case "$target" in all|hooks)
    mkdir -p "$stage/.claude/skills/ui-harness/references"
    env -u QUANTICK_BUBBLES cargo run -q -p quantick-app --features harness -- --dump-hook-registry >"$stage/.claude/skills/ui-harness/references/hook-registry.md"
esac

# Every producer must finish before any committed file can change. Compare
# bytes in check mode; in write mode publish each complete file by rename.
(cd "$stage" && find . -type f -print | LC_ALL=C sort) >"$stage.paths"
status=0
while IFS= read -r path; do
    path=${path#./}
    if cmp -s "$stage/$path" "$path"; then continue; fi
    if [ "$mode" = --check ]; then
        echo "generated artifact differs: $path" >&2
        status=1
    else
        mkdir -p "$(dirname -- "$path")"
        pending=$(mktemp "$(dirname -- "$path")/.regen.XXXXXX")
        cat "$stage/$path" >"$pending"
        mv -f "$pending" "$path"
        pending=
    fi
done <"$stage.paths"
exit "$status"
