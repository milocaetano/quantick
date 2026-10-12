#!/bin/sh
# Records `code-review-ok` from the reviewer's own verdict, so the implementer
# never writes the marker: Claude Code's auto-mode classifier denies that write.
# See README.md, "The `implement` tier".
#
# Wired as PostToolUse on SubagentHandback and as SubagentStop. A background
# code-review hands its report back through that tool; a foreground fork ends
# on it as its last message. Only a subagent's report is read, never the
# implementer's own text. The report's last line of the form
#
#   REVIEW-VERDICT: pass|fail <worktree> <reviewed HEAD>
#
# decides, and only while that worktree still sits at the reviewed HEAD and
# declares the implement tier: `pass` records the key pr-gate checks
# (`campaign_context.sh key`, the same diff hash), so a later commit leaves it
# stale; `fail` removes any record. Anything else changes nothing. Local
# workflow evidence, not a security boundary: CI and the user's merge protect
# main. Always exits 0.

input=$(cat)
flat=$(printf '%s' "$input" | tr -d '\r\n')

# The first string value of key `$1`.
field() {
    field_rest=${flat#*\"$1\"}
    [ "$field_rest" != "$flat" ] || return 0
    printf '%s' "$field_rest" |
        sed -n 's/^[[:space:]]*:[[:space:]]*"\(\([^"\\]\|\\.\)*\)".*/\1/p'
}

if [ "$(field hook_event_name)" = SubagentStop ]; then
    report=${flat#*\"last_assistant_message\"}
elif [ "$(field tool_name)" = SubagentHandback ]; then
    report=${flat#*\"tool_input\"}
else
    exit 0
fi
[ "$report" != "$flat" ] || exit 0

# JSON spells a Windows path C:\\src\\x and a line break \n: speak one slash,
# then split lines, so a path segment starting with n is never a break.
verdict=$(printf '%s' "$report" |
    sed 's/\\\\/\//g; s/\\[nrt]/\n/g' |
    sed -n 's/^.*REVIEW-VERDICT:[[:space:]]*\(pass\|fail\)[[:space:]]\{1,\}\([^"`[:space:]]\{1,\}\)[[:space:]]\{1,\}\([0-9a-fA-F]\{7,40\}\).*$/\1 \2 \3/p' |
    tail -n 1)
[ -n "$verdict" ] || exit 0
# shellcheck disable=SC2086 # three words, none of them globbed
set -f; set -- $verdict; set +f
outcome=$1 dir=$2 reviewed=$(printf '%s' "$3" | tr 'A-F' 'a-f')

[ -d "$dir" ] || exit 0
git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || exit 0
branch=$(git -C "$dir" symbolic-ref --quiet --short HEAD 2>/dev/null) || exit 0
[ "$(head -n 1 "$git_dir/mission-tier" 2>/dev/null | tr -d '\r')" = "$branch implement" ] || exit 0
head=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
case "$head" in "$reviewed"*) ;; *) exit 0 ;; esac

marker="$git_dir/code-review-ok"
if [ "$outcome" = fail ]; then
    rm -f "$marker"
    exit 0
fi
key=$(sh "$(dirname "$0")/campaign_context.sh" key "$dir" 2>/dev/null) || exit 0
[ -n "$key" ] || exit 0
printf '%s\n' "$key" > "$marker"
exit 0
