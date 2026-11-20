#!/bin/sh
# Every required job must return a successful verdict at the requested SHA.
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
mkdir "$root/bin"
checks=$(sed -n "s/^FULL_CI_CHECKS='\([^']*\)'.*/\1/p" "$script_dir/full_ci.sh")
case " $checks " in *' guards '*) ;; *) echo 'guards is not required'; exit 1 ;; esac
cat > "$root/bin/gh" <<'STUB'
#!/bin/sh
[ "$1" = api ] || exit 64
case "$2" in */commits/fixture-sha/check-runs\?*) ;; *) exit 64 ;; esac
check=${2##*check_name=}
check=${check%%&*}
if [ "$check" = "$FAILED_CHECK" ]; then
    printf '%s\n' "$VERDICT"
else
    printf 'completed:success\n'
fi
STUB
chmod +x "$root/bin/gh"
export FAILED_CHECK='' VERDICT='' PATH="$root/bin:$PATH"
sh "$script_dir/full_ci.sh" verify "$root" fixture-sha
for FAILED_CHECK in $checks; do
    export FAILED_CHECK
    for VERDICT in missing in_progress:none completed:failure completed:cancelled completed:skipped; do
        export VERDICT
        status=0
        output=$(sh "$script_dir/full_ci.sh" verify "$root" fixture-sha 2>&1) || status=$?
        if [ "$status" -ne 1 ]; then
            printf 'FAIL %s %s returned %s: %s\n' "$FAILED_CHECK" "$VERDICT" "$status" "$output"
            exit 1
        fi
        case "$output" in
            *"$FAILED_CHECK run"*) ;;
            *) printf 'FAIL wrong check: %s\n' "$output"; exit 1 ;;
        esac
    done
done
echo 'Full CI required-job verdict fixtures passed'
