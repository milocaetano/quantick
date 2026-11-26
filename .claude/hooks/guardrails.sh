#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Three CLAUDE.md rules were enforceable only by an agent remembering them.
# The first three modes make the harness enforce them instead. The fourth
# enforces nothing and is not a gate: it carries news from a check that
# already exists, and says so where it is defined.
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash. Denies an agent's write of
#                     `code-review-ok`, a `gh api` merge or un-draft, and a
#                     non-draft `gh pr
#                     create`, `gh pr ready` and `gh pr merge` until a
#                     current `code-review-ok` matches the exact change;
#                     an `implement` branch owes it before any PR, draft
#                     included. Ready and merge also want full CI (every
#                     job in FULL_CI_CHECKS) green at the exact HEAD, read
#                     by the sibling `full_ci.sh`: draft pushes run only
#                     `gate` and the fast job, so readiness is where it is
#                     owed (a prose-only head owes `gate` and `rust-skip`).
#   commit-reminder   PostToolUse on Bash. Cannot block (the commit
#                     already landed); says the gate is coming and how to
#                     satisfy it.
#   guard-watch       PostToolUse on Edit|Write. Not a gate: runs the
#                     repository guards over the file just written, using
#                     the already-built binary, and reports. It never
#                     denies and never blocks; `cargo test --workspace`
#                     remains the thing that enforces those guards.
#   review-verdict    PostToolUse on SubagentHandback, and SubagentStop.
#                     Records `code-review-ok` from a review subagent's
#                     verdict line, so the agent never writes it.
#   machine-check     SessionStart. Not a gate: when cargo on this machine
#                     has no `build.jobs` cap, tells the session to bring
#                     docs/dev-machine.md to the user before building.
#
# What `shell_statements` can and cannot see is listed where it is defined and
# in README.md; every spelling it claims has a case in guardrails_test.sh, and
# every spelling it does not is written down rather than half-handled.
#
# Every mode filters the tool payload itself rather than relying on the hook
# config's `if` matcher: a matcher that silently fails to match would leave a
# gate that looks armed and is not, and the filtering is covered by
# guardrails_test.sh either way.
#
# Fail-open by design: anything this script cannot determine exits 0 and the
# normal permission flow applies. A guardrail that blocks the session over its
# own bugs is worse than no guardrail, and the rules are also written down in
# CLAUDE.md.
#
# POSIX sh, no jq: it runs under Git Bash on Windows, under dash in CI.

set -u

# The branch that is never worked on directly, and the base every branch is
# cut from and measured against.
MAIN_BRANCH="main"
# The one review an `implement` branch owes, recorded after the code-review
# skill ran over the change and before any PR, draft included, is opened.
IMPLEMENT_MARKER="code-review-ok"

# What ceremony a branch declared, written by the `implement` skill into the
# per-branch git dir as `<branch> implement` and never committed. The name
# predates the retired `mission` skill. A declaration naming another branch
# is no declaration, so a reused worktree cannot inherit one.
TIER_FILE_NAME="mission-tier"
# How long the read-cost advisory may delay a command before it is dropped.
# The calculator is fast on an ordinary branch - a third of a second - but it
# walks every changed file's references, and the largest branch in the ledger,
# 408 changed files, takes 24 seconds. That is a stall in front of `gh pr
# create` for a line the pull request comment prints anyway, so past this
# budget the advice is dropped like anything else the advisory cannot
# determine in time. The budget is the whole advisory's, not each
# interpreter's: see `read_cost_advisory`.
READ_COST_ADVISORY_SECONDS=10
# The one tier the file may hold. Anything else is no declaration at all.
IMPLEMENT_TIER="implement"

mode="${1:-}"
input=$(cat)

# --- shared helpers ---------------------------------------------------------

# Pull one string field out of the hook's stdin JSON. Good enough for the flat
# fields we need (`file_path`, `command`, `cwd`); not a JSON parser.
json_string_field() {
    printf '%s' "$input" |
        tr -d '\n' |
        sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\(\([^\"\\\\]\|\\\\.\)*\)\".*/\1/p" |
        head -n 1
}

# JSON escapes a Windows path as C:\\src\\x. Undo that and speak one slash.
normalize_path() {
    printf '%s' "$1" | sed 's|\\\\|/|g; s|\\|/|g'
}

# Every file an edit tool targets. Claude's Write/Edit payloads carry one
# `file_path`; Codex's apply_patch payload carries the patch in `command`.
# Keep the translation here so both hosts reach the same policy functions.
edit_paths() {
    edit_file=$(json_string_field file_path)
    if [ -n "$edit_file" ]; then
        printf '%s\n' "$edit_file"
        return 0
    fi

    edit_patch=$(json_string_field command)
    [ -n "$edit_patch" ] || return 0
    printf '%s' "$edit_patch" |
        sed 's/\\r\\n/\
/g; s/\\n/\
/g' |
        sed -n 's/^\*\*\* \(Add\|Update\|Delete\) File: //p; s/^\*\*\* Move to: //p'
}

# Codex patch headers may be relative to the hook's cwd; Claude normally sends
# absolute file paths. Resolve both into the spelling the existing checks use.
absolute_edit_path() {
    edit_path=$(normalize_path "$1")
    case "$edit_path" in
        /* | ?:/*) printf '%s' "$edit_path" ;;
        *)
            edit_cwd=$(normalize_path "$(json_string_field cwd)")
            [ -d "$edit_cwd" ] || edit_cwd=$(normalize_path "$(pwd -P)")
            printf '%s/%s' "${edit_cwd%/}" "$edit_path"
            ;;
    esac
}

# Walk up to the first directory that exists: a Write targets a file that does
# not exist yet, and may create its parents too.
nearest_existing_dir() {
    d=$(dirname "$1")
    while [ ! -d "$d" ]; do
        p=$(dirname "$d")
        [ "$p" != "$d" ] || return 1
        d="$p"
    done
    printf '%s' "$d"
}

deny() {
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":%s}}\n' "$1"
    exit 0
}

context() {
    printf '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":%s}}\n' "$1"
    exit 0
}

# Neither a denial nor a silent pass: the gate could not determine something it
# is supposed to check, and says so where a human will read it.
#
# The file's fail-open rule is kept — `ask` blocks nothing a human does not
# block. What it refuses to do is fail open *silently*. An unreachable GitHub
# is the difference between "this branch has no open findings" and "nobody
# knows whether it has any", and a gate that prints the same nothing for both
# has taught its reader that silence means clean.
ask() {
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":%s}}\n' "$1"
    exit 0
}

# Raw JSON string `$1`, the payload's `command`, as the shell reads it: the
# escapes undone, so quotes and line breaks are real before anything splits.
json_command() {
    printf '%s' "$1" | awk '
        { s = s $0 }
        END {
            n = length(s)
            for (i = 1; i <= n; i++) {
                c = substr(s, i, 1)
                if (c != "\\" || i == n) { out = out c; continue }
                e = substr(s, ++i, 1)
                if (e == "n") out = out "\n"
                else if (e == "t") out = out "\t"
                else if (e == "r") out = out ""
                else if (e == "u") {
                    h = tolower(substr(s, i + 1, 4))
                    if (h == "0026") out = out "&"
                    else if (h == "003c") out = out "<"
                    else if (h == "003e") out = out ">"
                    else if (h == "0027") out = out "\047"
                    else { out = out "\\u" substr(s, i + 1, 4) }
                    i += 4
                }
                else out = out e
            }
            printf "%s", out
        }'
}

# Every statement the command `$1` would run, one per line, each starting at
# its command word. This is what the gate matches, so a gated command is seen
# wherever the shell would run it and nowhere it is merely mentioned:
#
#   split on `&&`, `||`, `;`, `|`, `&`, `(`, `)`, a backtick and a line break,
#     but never inside quotes (`$'...'` too), arithmetic `((...))` or a
#     redirect such as `>|`, `>&` or `&>`, so a commit message naming
#     `gh pr merge` stays one statement that runs `git`;
#   a heredoc body is data, inside `"$(cat <<EOF ...)"` too, unless a shell
#     or `source` named unquoted on the same line could read it; it is printed
#     as `#data <owner> <body>` for the caller's fallback;
#   control-flow keywords, `{`, `!`, `function NAME`, `coproc`, leading
#     redirects, the wrappers `time`, `nohup`, `winpty`, `exec`, `command`,
#     `builtin`, `sudo`, `env`, `timeout`, `nice`, `stdbuf` with their options,
#     option arguments and duration, and `VAR=value` words are stripped;
#   a command word spelled with quotes, `$'...'`, as a path or with `.exe` is
#     reduced to its lower-case name, `gh`'s flags before its subcommand move
#     after it, and `gh pr new` reads as `gh pr create`;
#   `bash -c '...'` (any shell, any option cluster with `c`), `env -S`,
#     `cmd /c`, `pwsh -c` and `eval` are unwrapped and their script split the
#     same way.
#
# A statement that writes the review marker also prints `#writes-marker`: a
# redirect to it, a file-writing tool or interpreter naming it, or an
# assignment naming it. The name is compared unquoted, without braces and
# case-insensitively, and a glob on the `code-review` stem counts.
#
# This is recognition, not proof: `pr_gate` asks rather than passes when a
# gated spelling survives in text this did not recognise as a statement.
shell_statements() {
    printf '%s' "$1" | awk '
        function trim(t) { sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t); return t }
        # Lower-cased: Windows resolves `GH.exe` to `gh.exe`.
        function base(w) {
            w = tolower(w)
            sub(/^.*[\/\\]/, "", w)
            if (substr(w, length(w) - 3) == ".exe") w = substr(w, 1, length(w) - 4)
            return w
        }
        function is_shell(w) { return w ~ /^(sh|bash|dash|zsh|ksh|ash)$/ }
        # The character that closes quote state `q`; `$` is `$'"'"'...'"'"'`.
        function closer(q) { return q == "$" ? "\047" : q }
        # Split `t` into words[1..nw], quotes kept, quote-aware.
        function words_of(t,    i, n, c, q, w, nw) {
            delete words
            n = length(t); q = ""; w = ""; nw = 0
            for (i = 1; i <= n; i++) {
                c = substr(t, i, 1)
                if (q != "") {
                    if (q != "\047" && c == "\\" && i < n) { w = w c substr(t, ++i, 1); continue }
                    if (c == closer(q)) q = ""
                    w = w c; continue
                }
                if (c == "\\" && i < n) { w = w c substr(t, ++i, 1); continue }
                if (c == "$" && substr(t, i + 1, 1) == "\047") { q = "$"; w = w c "\047"; i++; continue }
                if (c == "\047" || c == "\"") { q = c; w = w c; continue }
                if (c == " " || c == "\t") { if (w != "") words[++nw] = w; w = ""; continue }
                w = w c
            }
            if (w != "") words[++nw] = w
            return nw
        }
        # What the shell passes for word `w`: quotes removed, escapes applied.
        # `$'"'"'...'"'"'` keeps only what its escapes spell literally.
        function unquote(w,    i, n, c, q, out) {
            n = length(w); q = ""; out = ""
            for (i = 1; i <= n; i++) {
                c = substr(w, i, 1)
                if (q == "\047") { if (c == q) q = ""; else out = out c; continue }
                if (q == "$") {
                    if (c == "\\" && i < n) { out = out substr(w, ++i, 1); continue }
                    if (c == "\047") q = ""; else out = out c
                    continue
                }
                if (q == "\"") {
                    if (c == "\\" && i < n && index("\"\\$`", substr(w, i + 1, 1))) { out = out substr(w, ++i, 1); continue }
                    if (c == q) q = ""; else out = out c
                    continue
                }
                if (c == "\\" && i < n) { out = out substr(w, ++i, 1); continue }
                if (c == "$" && substr(w, i + 1, 1) == "\047") { q = "$"; i++; continue }
                if (c == "\047" || c == "\"") { q = c; continue }
                out = out c
            }
            return out
        }
        function names_marker(w) {
            w = tolower(unquote(w)); gsub(/[{},]/, "", w)
            return w ~ /code-review-ok/ || w ~ /code-review[^ ]*[*?[]/
        }
        function is_redirect(w) { return w ~ /^[0-9]*(&?>>?|>[|&]|<&?|<>|&>>?)/ }
        # True when words[k..nw] write the marker.
        function writes_marker(k, nw,    j, w, u, p, tgt, b, tool, inplace) {
            for (j = k; j <= nw; j++) {
                w = words[j]
                gsub("\"[^\"]*\"|\047[^\047]*\047", "", w)
                if (!index(w, ">")) continue
                u = unquote(words[j]); p = index(u, ">")
                tgt = substr(u, p + 1); sub(/^[>|&]/, "", tgt)
                if (tgt == "" && j < nw) tgt = words[j + 1]
                if (names_marker(tgt)) return 1
            }
            b = base(unquote(words[k]))
            tool = b ~ /^(tee|cp|mv|install|ln|touch|dd|truncate|rsync|curl|wget|sort|tar|unzip|cpio|python|python3|py|perl|ruby|node|pwsh|powershell|cmd)$/
            for (j = k + 1; j <= nw; j++) if (b == "sed" && words[j] ~ /^(-[a-z]*i|--in-place)/) inplace = 1
            if (!tool && !inplace) return 0
            for (j = k + 1; j <= nw; j++) if (names_marker(words[j])) return 1
            return 0
        }
        # The words after words[j] as one script, for an unwrapped wrapper.
        function tail_from(j, nw,    rest) {
            rest = ""
            for (; j <= nw; j++) rest = rest " " words[j]
            return rest
        }
        # Print statement `t` from its command word, or split what it unwraps.
        function emit(t, depth,    nw, k, w, b, o, cmd, j, j0, rest, opts, assign) {
            t = trim(t)
            if (t == "") return
            nw = words_of(t)
            k = 1; assign = 0
            while (k <= nw) {
                w = unquote(words[k]); b = base(w)
                if (w ~ /^(\{|!|if|then|else|elif|do|while|until|coproc)$/) { k++; continue }
                if (w == "function") { k += 2; continue }
                if (is_redirect(words[k])) {
                    if (names_marker(words[k]) || (words[k] ~ />$/ && names_marker(words[k + 1]))) assign = 1
                    k += (words[k] ~ /[<>&]$/) ? 2 : 1
                    continue
                }
                if (w ~ /^[A-Za-z_][A-Za-z0-9_]*=/) { if (names_marker(w)) assign = 1; k++; continue }
                if (b ~ /^(time|nohup|winpty|exec|command|builtin|sudo|env|timeout|nice|stdbuf)$/) {
                    k++
                    while (k <= nw && words[k] ~ /^-/) {
                        o = words[k]; k++
                        if (o == "--") break
                        if (b == "env" && (o ~ /^-S/ || o ~ /^--split-string/)) {
                            if (o == "-S" || o == "--split-string") rest = unquote(words[k++])
                            else { rest = o; sub(/^(-S|--split-string=)/, "", rest); rest = unquote(rest) }
                            if (depth < 8) split_all(rest tail_from(k, nw), depth + 1)
                            return
                        }
                        if (o ~ /^--[a-z-]+$/ && o ~ /^--(unset|chdir|signal|kill-after|user|group|adjustment|input|output|error|prompt|other-user|close-from|host)$/) k++
                        else if ((b == "env" && o ~ /^-[uCP]$/) || (b == "exec" && o == "-a") ||
                            (b == "sudo" && o ~ /^-[ugCDhprtU]$/) || (b == "timeout" && o ~ /^-[sk]$/) ||
                            (b == "nice" && o == "-n") || (b == "stdbuf" && o ~ /^-[ioe]$/)) k++
                    }
                    if (b == "timeout" && k <= nw) k++
                    continue
                }
                break
            }
            if (assign) print "#writes-marker"
            # Nothing but assignments: the statement itself, as written.
            if (k > nw) { print t; return }
            if (writes_marker(k, nw)) print "#writes-marker"
            cmd = base(unquote(words[k]))
            if (depth < 8 && is_shell(cmd)) {
                for (j = k + 1; j <= nw; j++) {
                    if (words[j] ~ /^-[A-Za-z]*c[A-Za-z]*$/) {
                        if (words[j + 1] == "--") j++
                        if (j < nw) split_all(unquote(words[j + 1]), depth + 1)
                        return
                    }
                    if (words[j] ~ /^([-+]o|[-+]O|--rcfile|--init-file)$/) { j++; continue }
                    if (words[j] !~ /^[-+]/) break
                }
            }
            # Windows shells: `cmd /c ...` and `pwsh -c ...` run their rest.
            if (depth < 8 && cmd == "cmd")
                for (j = k + 1; j <= nw; j++)
                    if (tolower(unquote(words[j])) ~ /^\/\/?[ck]$/) { split_all(unquote(tail_from(j + 1, nw)), depth + 1); return }
            if (depth < 8 && cmd ~ /^(pwsh|powershell)$/)
                for (j = k + 1; j <= nw; j++)
                    if (tolower(words[j]) ~ /^-(c|command)$/) { split_all(unquote(tail_from(j + 1, nw)), depth + 1); return }
            if (depth < 8 && cmd == "eval") {
                rest = ""
                for (j = k + 1; j <= nw; j++) rest = rest " " unquote(words[j])
                split_all(rest, depth + 1)
                return
            }
            # `gh -R o/r pr merge` runs `gh pr merge`: its flags move behind
            # the subcommand, kept, so a pinned form still sees them.
            j0 = k + 1; opts = ""
            if (cmd == "gh")
                while (j0 <= nw && words[j0] ~ /^-/) {
                    opts = opts " " words[j0]
                    if (words[j0] ~ /^(-R|--repo|--hostname)$/ && j0 < nw) opts = opts " " words[++j0]
                    j0++
                }
            # The subcommand words unquoted, so `gh "pr" merge` reads as what
            # it runs; every argument after them stays as written, quotes kept.
            rest = cmd
            for (j = j0; j <= nw && j <= j0 + 1; j++) {
                w = words[j]
                if (unquote(w) !~ /[ \t]/) w = unquote(w)
                # gh'"'"'s own alias for `gh pr create`.
                if (cmd == "gh" && j == j0 + 1 && w == "new" && rest == "gh pr") w = "create"
                rest = rest " " w
            }
            rest = rest opts
            for (j = j0 + 2; j <= nw; j++) rest = rest " " words[j]
            print rest
        }
        # Record a heredoc opened at `s[i]` by the statement `owner`; return
        # the index after its delimiter word, which is unquoted as the shell does.
        function heredoc(s, i, n, depth, owner,    j, ds, dw, c, q) {
            j = i + 2; ds = 0
            if (substr(s, j, 1) == "-") { ds = 1; j++ }
            while (substr(s, j, 1) == " " || substr(s, j, 1) == "\t") j++
            dw = ""; q = ""
            for (; j <= n; j++) {
                c = substr(s, j, 1)
                if (q != "") { if (c == q) q = ""; dw = dw c; continue }
                if (c == "\\" && j < n) { dw = dw c substr(s, ++j, 1); continue }
                if (c == "\047" || c == "\"") { q = c; dw = dw c; continue }
                if (c ~ /[ \t\n;&|<>()]/) break
                dw = dw c
            }
            dw = unquote(dw)
            if (dw != "") {
                hdn[depth]++
                delim[depth, hdn[depth]] = dw; dstrip[depth, hdn[depth]] = ds
                downer[depth, hdn[depth]] = owner
            }
            return j
        }
        # Skip the pending heredoc bodies after the newline at `s[i]`, printing
        # each as `#data <owner> <body>`.
        function skip_bodies(s, i, n, depth,    d, e, line, body) {
            for (d = 1; d <= hdn[depth]; d++) {
                body = ""
                while (i < n) {
                    e = index(substr(s, i + 1), "\n")
                    line = e ? substr(s, i + 1, e - 1) : substr(s, i + 1)
                    i = e ? i + e : n
                    if (dstrip[depth, d]) sub(/^\t+/, "", line)
                    if (line == delim[depth, d]) break
                    body = body " " line
                }
                print "#data " downer[depth, d] body
            }
            hdn[depth] = 0
            return i
        }
        # The command word a statement in progress would run, for a heredoc.
        function owner_of(t,    nw, k) {
            nw = words_of(trim(t))
            for (k = 1; k <= nw && unquote(words[k]) ~ /^([A-Za-z_][A-Za-z0-9_]*=|\{|!|if|then|do|while|until)/; k++) ;
            return k <= nw ? base(unquote(words[k])) : "-"
        }
        # Arithmetic: return the index of the `)` closing the `((` at `s[i]`.
        function arith_end(s, i, n,    j, p) {
            p = 0
            for (j = i; j <= n; j++) {
                if (substr(s, j, 1) == "(") p++
                else if (substr(s, j, 1) == ")" && --p == 0) return j
            }
            return n
        }
        function split_all(s, depth,    n, i, c, q, cur, uq, line_shell, j) {
            n = length(s); q = ""; cur = ""; uq = ""; line_shell = 0; hdn[depth] = 0
            for (i = 1; i <= n; i++) {
                c = substr(s, i, 1)
                if (q == "\047") { if (c == q) q = ""; cur = cur (c == "\n" ? " " : c); continue }
                if (q == "$") {
                    if (c == "\\" && i < n) { cur = cur c substr(s, ++i, 1); continue }
                    if (c == "\047") q = ""
                    cur = cur (c == "\n" ? " " : c); continue
                }
                if (q == "\"") {
                    if (c == "\\" && i < n) { cur = cur c substr(s, ++i, 1); continue }
                    # `"$(cat <<EOF ...)"`: the body is the message, never
                    # quotes. Only inside an open `$(`: a quoted `a << b` is text.
                    if (substr(s, i, 2) == "<<" && substr(s, i, 3) != "<<<" && cur ~ /\$\([^)]*$/) {
                        j = heredoc(s, i, n, depth, owner_of(cur)); cur = cur substr(s, i, j - i); i = j - 1; continue
                    }
                    if (c == "\n" && hdn[depth] > 0) { i = skip_bodies(s, i, n, depth); cur = cur " "; continue }
                    if (c == q) q = ""
                    cur = cur (c == "\n" ? " " : c); continue
                }
                if (c == "\\" && i < n) {
                    if (substr(s, i + 1, 1) == "\n") { i++; cur = cur " "; continue }
                    cur = cur c substr(s, ++i, 1); uq = uq " "; continue
                }
                if (c == "$" && substr(s, i + 1, 1) == "\047") { q = "$"; cur = cur c "\047"; uq = uq " "; i++; continue }
                if (c == "\047" || c == "\"") { q = c; cur = cur c; uq = uq " "; continue }
                # Arithmetic is one word: `$((1<<1))` and `((a<<2))` open no heredoc.
                if (substr(s, i, 3) == "$((" || (substr(s, i, 2) == "((" && trim(cur) == "")) {
                    j = arith_end(s, i + (c == "$"), n)
                    cur = cur substr(s, i, j - i + 1); uq = uq " "; i = j; continue
                }
                if (c == "#" && (cur == "" || cur ~ /[ \t]$/)) {
                    while (i < n && substr(s, i + 1, 1) != "\n") i++
                    continue
                }
                if (substr(s, i, 2) == "<<" && substr(s, i, 3) != "<<<") {
                    j = heredoc(s, i, n, depth, owner_of(cur)); cur = cur substr(s, i, j - i); i = j - 1; continue
                }
                # `>|`, `>&`, `<&` and `&>` are redirects, not separators.
                if ((c == "|" || c == "&") && cur ~ /[<>]$/ || (c == "&" && substr(s, i + 1, 1) == ">")) {
                    cur = cur c; uq = uq c; continue
                }
                if (c ~ /[;|&()`\n]/) {
                    if (uq ~ /(^|[ \/\\])(sh|bash|dash|zsh|ksh|ash|source|\.)(\.exe)?([ \t]|$)/) line_shell = 1
                    emit(cur, depth); cur = ""; uq = ""
                    if (c == "\n") {
                        # A body a shell may read is commands; any other is data.
                        if (hdn[depth] > 0 && !line_shell) i = skip_bodies(s, i, n, depth)
                        hdn[depth] = 0; line_shell = 0
                    }
                    continue
                }
                cur = cur c; uq = uq c
            }
            emit(cur, depth)
        }
        { s = s (NR > 1 ? "\n" : "") $0 }
        END { split_all(s, 0) }'
}

# True when the command actually runs `$2` as a statement, rather than merely
# mentioning it. A commit message body reaches the hook inside the command
# string, so a free substring match blocks `git commit -F -` whenever the
# message happens to name the gated command.
runs_command() {
    shell_statements "$1" | grep -qE "^$2([[:space:]]|\$)"
}

# The directory the command operates on. The payload's `cwd` is the session's,
# which resets between calls, while every agent command reaches its worktree
# with a leading `cd <dir> &&`. Trusting `cwd` alone made the gate judge the
# main checkout no matter which branch was being shipped.
effective_dir() {
    # The first line only: a `cd` on a later line runs after the first line's
    # statements, so it cannot say where they ran.
    d=$(printf '%s\n' "$1" | head -n 1 | sed -n 's|^[[:space:]]*cd[[:space:]][[:space:]]*"\{0,1\}\([^"[:space:];&|]*\)"\{0,1\}.*|\1|p' | head -n 1)
    d=$(normalize_path "$d")
    if [ -n "$d" ] && [ -d "$d" ]; then
        printf '%s' "$d"
        return 0
    fi
    printf '%s' "$2"
}

# The command with its single leading `cd <dir> &&` removed, trimmed at both
# ends. Everything else is left exactly as written.
#
# The integration merge and ready statements below are pinned by string equality, and the function
# above says why every agent command carries that prefix: the payload's `cwd`
# is the session's and resets between calls, so the bare form the pin asks for
# would be judged against the main checkout and denied as a merge to main. The
# pinned form was therefore unreachable from the Bash tool in both directions.
# The prefix is read with the pattern `effective_dir` uses, so the directory
# the gate judges and the directory it strips can never disagree.
#
# Exactly one prefix, and only across `&&`. A second `cd`, a `;`, a `||` or any
# trailing statement survives the strip and fails the equality check that
# follows, so nothing beside the first statement can share its authorization.
bare_statement() {
    bare_command=$(printf '%s' "$1" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    bare_rest=$(printf '%s' "$bare_command" |
        sed -n 's|^cd[[:space:]][[:space:]]*"\{0,1\}[^"[:space:];&|]\{1,\}"\{0,1\}[[:space:]]*&&[[:space:]]*||p')
    if [ -n "$bare_rest" ]; then
        printf '%s' "$bare_rest"
        return 0
    fi
    printf '%s' "$bare_command"
}

# Empty when the git dir cannot be resolved, so the caller can fail open
# rather than build a path rooted at `/` and hand back a remedy telling an
# agent to write the marker at the filesystem root.
marker_path() {
    marker_git_dir=$(git -C "$1" rev-parse --absolute-git-dir 2>/dev/null) || return 1
    [ -n "$marker_git_dir" ] || return 1
    printf '%s/%s' "$marker_git_dir" "$2"
}

# The tier declared for worktree `$1`, or nothing when none is declared or the
# declaration is not `$IMPLEMENT_TIER`. The file's contents never leave this
# function: an unrecognised tier is reported as no tier at all, so a corrupt or
# hostile file can neither reach `deny`'s JSON nor widen anything.
declared_tier() {
    tier_file=$(marker_path "$1" "$TIER_FILE_NAME") || return 1
    [ -f "$tier_file" ] || return 1

    tier_branch=$(git -C "$1" rev-parse --abbrev-ref HEAD 2>/dev/null) || return 1
    [ -n "$tier_branch" ] || return 1

    tier_line=$(head -n 1 "$tier_file" 2>/dev/null |
        tr -d '\r' |
        tr '\t' ' ' |
        sed 's/^ *//; s/ *$//; s/  */ /g')

    # Exactly two fields, and the code says so rather than approximating it.
    # One field is the format this file used before it carried a branch, and
    # reading it as a tier would restore the inheritance bug the format exists
    # to close, so it is refused rather than migrated. Three fields is a typo,
    # and it is refused here rather than incidentally by the branch compare
    # below — a later edit that relaxes that compare must not quietly start
    # accepting a shape this comment promises is rejected.
    case "$tier_line" in
        *' '*' '*) return 1 ;;
        *' '*) ;;
        *) return 1 ;;
    esac

    tier_for=${tier_line% *}
    tier=$(printf '%s' "${tier_line##* }" | tr 'A-Z' 'a-z')

    # `rev-parse --abbrev-ref` prints the literal string `HEAD` when the head is
    # detached, and the snippet that *writes* this file uses that same command -
    # so a declaration made while detached records `HEAD`, and comparing it here
    # would then match every future detached checkout in this worktree. That is
    # the inheritance bug the branch field exists to close, wearing a different
    # hat, and it was reproduced against the version of this function that
    # merely claimed detached heads "grant nothing". A detached head names no
    # branch, so it declares nothing.
    [ "$tier_branch" != "HEAD" ] || return 1

    # A declaration belongs to one branch.
    [ "$tier_for" = "$tier_branch" ] || return 1

    [ "$tier" = "$IMPLEMENT_TIER" ] || return 1
    printf '%s' "$tier"
}

# The identity of what a review covered: a hash of this branch's own diff
# against origin/<main>, not the sha of whichever commit happened to carry it.
# Empty when it cannot be computed, and the caller then fails open exactly as it
# did when a `rev-parse` failed.
#
# The change is strictly tightening one case while relaxing another that never
# mattered:
#
#   rebase, amend, reword         same change -> holds  (it used to break)
#   origin/main moves, no rebase  same change -> holds  (the merge base is put)
#   any edit to a tracked file    new change  -> stales, as before
#   upstream edits a file this    new change  -> stales, which the sha form
#     branch also touches                        did NOT catch
#
# The last row is the point. A sha-keyed marker survives a rebase that lands the
# branch on top of someone else's edits to the very files it changes, which is
# the case most deserving of a second look. A diff-keyed one does not.
review_base() {
    base_record=$(marker_path "$1" mission-base) || return 1
    if [ -e "$base_record" ]; then
        sh "$(dirname "$0")/mission_base.sh" base "$1"
    else
        printf 'origin/%s\n' "$MAIN_BRANCH"
    fi
}

review_key() {
    key_record=$(marker_path "$1" mission-base) || return 1
    if [ -e "$key_record" ]; then
        sh "$(dirname "$0")/mission_base.sh" key "$1"
        return $?
    fi
    # The preconditions are checked first, and the diff is then piped *raw*.
    # Capturing it in $( ) first would strip the trailing newline, so this
    # function and the recording command the denial prints - a plain
    # `git diff ... | git hash-object --stdin` - would hash different bytes and
    # the gate could never be satisfied. Same pipeline on both sides, always.
    git -C "$1" rev-parse --verify --quiet "origin/$MAIN_BRANCH" >/dev/null 2>&1 || return 1
    git -C "$1" merge-base "origin/$MAIN_BRANCH" HEAD >/dev/null 2>&1 || return 1
    git -C "$1" diff "origin/$MAIN_BRANCH...HEAD" 2>/dev/null |
        git -C "$1" hash-object --stdin 2>/dev/null
}

# Deny unless marker `$3` in worktree `$1` records exactly the change `$2`
# being shipped. `$4` states the rule in CLAUDE.md's own words, `$5` says how
# to satisfy it, and the recording line is derived from the marker name — so
# the instruction and the file the gate reads cannot drift apart.
#
# The marker is meant to hold a sha and nothing enforces that: a mistyped
# redirect, an editor appending a line, a half-written file. Its contents are
# reduced to hex and lower-cased before they reach `deny`, which interpolates
# them into a JSON string — an unescaped quote or a raw newline there emits a
# payload the harness cannot parse, and a *lost* deny decision turns the gate
# off rather than tripping it.
require_marker() {
    require_dir=$1
    require_key=$2
    require_name=$3
    require_rule=$4
    require_how=$5

    require_file=$(marker_path "$require_dir" "$require_name") || exit 0
    # `git -C "$require_dir"`, never `git -C .`. The remedy is pasted into a
    # shell whose cwd is the session's — the main checkout — not the worktree
    # being shipped. With `.` the marker lands in the *shared* git dir holding
    # main's HEAD, where a later command resolving to the main checkout would
    # match it, so one paste of the gate's own instruction could switch it off.
    # Every doc on this branch spells the command with the `cd`; the message
    # must not be the one place that drops it.
    require_record="git -C \\\"$require_dir\\\" diff origin/$MAIN_BRANCH...HEAD | git hash-object --stdin > \\\"$require_file\\\""
    if [ -e "$(marker_path "$require_dir" mission-base)" ]; then
        require_record="sh .claude/hooks/mission_base.sh key \\\"$require_dir\\\" > \\\"$require_file\\\""
    fi
    if [ "$require_name" = "$IMPLEMENT_MARKER" ]; then
        # The agent never writes this marker, and pr-gate refuses the write;
        # the review-verdict hook records it, and under Codex, which has no
        # such hook, the user does.
        require_record="end the review with \`REVIEW-VERDICT: pass $require_dir <HEAD>\` and the review-verdict hook records it. Codex: ask the user to run, outside the agent: $require_record"
    fi

    if [ ! -f "$require_file" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` has not been recorded for this change. $require_how, then record it:\n\n  $require_record\""
    fi

    # Trim only what a file legitimately picks up — a trailing CR, a BOM,
    # surrounding blanks — then require the remainder to be exactly a sha.
    # Stripping every non-hex byte instead would silently accept `<sha> ok`
    # or a sha with a comment beside it, which a verbatim compare rejects.
    require_reviewed=$(head -n 1 "$require_file" 2>/dev/null |
        tr -d '\r' |
        sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    case "$require_reviewed" in
        '' | *[!0-9a-fA-F]*) require_reviewed="(not a commit id)" ;;
        *)
            [ ${#require_reviewed} -eq ${#require_key} ] ||
                require_reviewed="(not a commit id)"
            ;;
    esac
    require_reviewed=$(printf '%s' "$require_reviewed" | tr 'A-F' 'a-f')

    if [ "$require_reviewed" != "$require_key" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` was recorded for change $require_reviewed but this branch's diff is now $require_key, so the newest work is ungraded. A rebase, an amend or a reword does not move this value; an edit to a tracked file does. $require_how, then record it again:\n\n  $require_record\""
    fi
}

# --- worktree-guard ---------------------------------------------------------

worktree_guard() {
    # An explicit opt-out for the rare deliberate edit on the main checkout: a
    # hotfix, a repo-hygiene sweep. Set it in the shell before launching.
    if [ "${QUANTICK_ALLOW_MAIN_WRITES:-}" = "1" ]; then
        exit 0
    fi

    paths=$(edit_paths)
    [ -n "$paths" ] || exit 0
    old_ifs=$IFS
    IFS='
'
    for raw in $paths; do
        path=$(absolute_edit_path "$raw")

        # The live goal is agent working state and is deliberately written
        # before the branch artifact exists. Tracked skills and hooks are not
        # exempt: they belong in the worktree like every other repository edit.
        case "$path" in
            */.claude/GOAL.md) continue ;;
        esac

        dir=$(nearest_existing_dir "$path") || continue

        git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || continue
        common_dir=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || continue

        # In a linked worktree these differ (<common>/worktrees/<name> vs
        # <common>). Equal means the write lands in the main checkout.
        [ "$git_dir" = "$common_dir" ] || continue

        branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || continue
        [ "$branch" = "$MAIN_BRANCH" ] || continue

        deny "\"CLAUDE.md: one goal, one worktree. This write lands in the main checkout while it is on \`$MAIN_BRANCH\`. Create the worktree first:\n\n  git fetch origin\n  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/$MAIN_BRANCH\n\nthen work inside that directory. Set QUANTICK_ALLOW_MAIN_WRITES=1 to override deliberately.\""
    done
    IFS=$old_ifs
    exit 0
}

# --- pr-gate ----------------------------------------------------------------

# The statement among `shell_statements` output `$1` that runs `$2`, or
# nothing. A flag the gate reads must come from the statement it matched
# rather than from anywhere else on the line: a `--draft` in a neighbouring
# `echo` is not a draft PR.
gh_statement() {
    printf '%s\n' "$1" |
        grep -E "^$2([[:space:]]|$)" |
        head -n 1
}

# True when statements `$1` run `$2`.
has_statement() {
    printf '%s\n' "$1" | grep -qE "^$2([[:space:]]|\$)"
}

# True when statements `$1` write the review marker, as `shell_statements`
# judged it. Reading or removing it stays allowed; removing only makes the
# gate stricter.
writes_marker() {
    printf '%s\n' "$1" | grep -qx '#writes-marker'
}

# True when statements `$1`, outside those already judged and outside `git`
# statements and their heredocs, still spell a gated call or a marker write
# once flattened. See its use in `pr_gate`.
unknown_gated() {
    printf '%s\n' "$1" |
        grep -vE '^(git( |$)|#data git |#writes-marker$|gh pr (merge|ready|create)( |$)|gh api( |$))' |
        tr -d "'\"\\\\\$\`{}," | tr 'A-Z\t' 'a-z ' |
        sed 's/[0-9]*>>* *\/dev\/null//g; s/[0-9]*>&[0-9]//g' |
        grep -qE 'gh +pr +(merge|ready|create|new)|gh +api.*(/merge|graphql|draft)|code-review-ok.*(>|(^| )(tee|cp|mv|install|ln|touch|dd|truncate|rsync|curl|wget|sort|tar|unzip|cpio|python3?|py|perl|ruby|node|pwsh|powershell|cmd|exec)( |$)|sed .*-i)|((>|(^| )(tee|cp|mv|install|ln|touch|dd|truncate|rsync|curl|wget|sort|tar|unzip|cpio|python3?|py|perl|ruby|node|pwsh|powershell|cmd|exec) ).*code-review-ok)'
}

# True when a `gh api` statement in `$1` merges, enqueues or un-drafts a PR: a
# PUT to REST `pulls/<n>/merge`; a `draft=` field, or a PATCH with `--input`,
# on `pulls/<n>`; or a GraphQL call (`graphql`, `/graphql` or a URL) naming
# the merge, auto-merge, enqueue or ready mutations, or taking its query from
# a file or an expansion the gate cannot read. Matched with quotes removed, so
# `merge''PullRequest` is the mutation it spells. Reads stay ungated.
api_ships_pr() {
    printf '%s\n' "$1" | awk '
        /^gh api([ \t]|$)/ {
            s = tolower($0); gsub(/["\047\\]/, "", s)
            if (s ~ /[ \t](https?:\/\/[^ \t]*)?\/?graphql([ \t?]|$)/ && s ~ /mergepullrequest|enablepullrequestautomerge|enqueuepullrequest|markpullrequestreadyforreview|--input|query=@|\$/) hit = 1
            pull = s ~ /pulls\/[^\/ \t"\047?]+([ \t"\047?]|$)/
            if (s ~ /pulls\/[^\/ \t]+\/merge/ && s ~ /(-x|--method)[= ]?["\047]?put/) hit = 1
            if (pull && s ~ /(-f|--field|--raw-field)[= ]?["\047]?draft=/) hit = 1
            if (pull && s ~ /--input/ && s ~ /(-x|--method)[= ]?["\047]?patch/) hit = 1
        }
        END { exit !hit }'
}

# The statement with its quoted spans blanked out, so a flag can be read from
# what the command *does* rather than from what a title or a body says.
#
# This exists because the first version of the draft exemption globbed the
# whole statement, and `gh pr create --title "Make --draft PRs ungated"` then
# opened a real, un-reviewed PR. The file already carries that lesson one
# function up: `runs_command` splits into statements precisely so a commit
# message naming a gated command cannot trip the gate. Reading a *flag* needs
# the same care, and more of it — there the mistake blocks something that
# should pass, here it passes something that should be blocked.
#
# A title with a space in it has to be quoted, so blanking quoted spans covers
# the reachable spellings. It is deliberately not a shell parser: what it
# cannot understand it leaves in place, and anything left in place is read as
# an argument, which errs towards gating.
unquoted_arguments() {
    printf '%s' "$1" | sed 's/"[^"]*"/ /g; s/'"'"'[^'"'"']*'"'"'/ /g'
}

# True when the statement asks for a draft PR, as a whole argument word.
#
# Every uncertainty here resolves to *not a draft*, because the two mistakes
# are not the same size: reading a draft as a real PR costs a permission
# prompt, while reading a real PR as a draft opens it with no review at all.
draft_flag() {
    # An odd count of either quote character means a span this cannot pair
    # off, so `unquoted_arguments` cannot be trusted to have blanked the right
    # text. Refuse rather than guess.
    for draft_quote in '"' "'"; do
        draft_quotes=$(printf '%s' "$1" | tr -cd "$draft_quote" | wc -c)
        [ $((draft_quotes % 2)) -eq 0 ] || return 1
    done

    draft_words=$(unquoted_arguments "$1")

    # `--draft=<value>` is a draft only when gh would read the value as true.
    # gh parses it with Go's ParseBool, whose true spellings are exactly these
    # six — so `--draft=f`, `--draft=F` and `--draft=False` are all *false*,
    # and a gate that reads them as a draft opens a real, un-reviewed PR. Only
    # `false` and `0` were excluded before, which left the other four spellings
    # of the same instruction wide open.
    case " $draft_words " in
        *' --draft=1 '* | *' --draft=t '* | *' --draft=T '* | \
        *' --draft=true '* | *' --draft=TRUE '* | *' --draft=True '*) return 0 ;;
        # Any other explicit value: not a draft, whatever it says.
        *' --draft='*) return 1 ;;
    esac

    case " $draft_words " in
        *' --draft '* | *' -d '*) return 0 ;;
    esac
    return 1
}

# The PR number the statement names, or nothing.
#
# A *whole word* of digits, never a digit run pulled out of one: splitting on
# every non-digit read `--body-file notes2.md 42` as PR 2. And exactly one such
# word, or none — two bare numbers mean the operand cannot be told from a
# flag's value, and a gate that counts the wrong PR's threads is worse than one
# that counts none, because it reports a clean number for a branch nobody
# reviewed. The caller turns "none" into a question rather than a pass.
pr_number() {
    number_words=$(unquoted_arguments "$1" | tr -s ' \t' '\n\n' | grep -E '^[0-9]+$')
    [ "$(printf '%s\n' "$number_words" | grep -c '[0-9]')" -eq 1 ] || return 0
    printf '%s' "$number_words"
}

# --- the read-cost advisory -------------------------------------------------

# Not a gate, and it lives inside `pr-gate` only because this is the moment a
# branch is finished enough for the number to mean something.
#
# `docs/quality/read-cost/ledger.md` records what each merged pull request
# asked a reader to hold, and carries the ceiling those rows produced: the
# median of the feature branches, rounded to the thousand. A feature branch
# over it hears about it once, here, while detaching a reference is still
# cheap, and hears it as news. Nothing on this path can refuse anything.
#
# Silence answers everything it cannot determine - no interpreter, no
# calculator in the worktree, no recorded ceiling, a measurement that fails, a
# branch the ceiling was never about. That is not the file's fail-open rule
# being stretched: advice that reports its own plumbing teaches its reader to
# skip the line that matters.
#
# Each interpreter is tried until one *runs*, rather than until one exists.
# On the Windows checkout this hook runs on, `python3` is on PATH as the
# Microsoft Store alias: it resolves, prints "Python was not found" and exits
# 49. Stopping at the first name found meant the trader's own machine — the
# one machine this advice is for — silently never saw it.
read_cost_advisory() {
    advisory_script="$1/tools/read_cost/report.py"
    [ -f "$advisory_script" ] || return 0
    advisory_branch=$(git -C "$1" symbolic-ref --quiet --short HEAD 2>/dev/null) ||
        return 0
    # The interpreter is chosen before the measurement, not by attempting it.
    # Choosing by attempt gave every interpreter its own budget: a branch that
    # exhausted the first one was killed, read as "that interpreter did not
    # run", and measured again under the second, so a 20-second cap delayed a
    # command by 40.
    advisory_python=
    for advisory_candidate in python3 python; do
        command -v "$advisory_candidate" >/dev/null 2>&1 || continue
        "$advisory_candidate" -c "" >/dev/null 2>&1 || continue
        advisory_python=$advisory_candidate
        break
    done
    [ -n "$advisory_python" ] || return 0

    advisory_limit=
    command -v timeout >/dev/null 2>&1 &&
        advisory_limit="timeout $READ_COST_ADVISORY_SECONDS"

    $advisory_limit "$advisory_python" "$advisory_script" warn \
        --repo "$1" --base "$2" --head HEAD \
        --branch "$advisory_branch" 2>/dev/null
    return 0
}

# Leave the gate without a decision, carrying whatever the advisory has to
# say. `systemMessage` is what a person sees and `additionalContext` is what
# the session reads; both hold the same sentence, and neither is a permission
# decision, so the normal flow continues exactly as it did before.
#
# The measurement happens here, on the way past, rather than when the gate
# starts. A denial says its own thing and the advice returns on the attempt
# that passes, so measuring earlier would only put the calculator's seconds in
# front of a command that was about to be refused anyway.
#
# An invalid review base is the gate's business, not the advisory's: with
# nothing to measure against it simply says nothing.
pass_pr_gate() {
    if pass_base=$(review_base "$1" 2>/dev/null); then
        pass_note=$(read_cost_advisory "$1" "$pass_base")
        if [ -n "$pass_note" ]; then
            printf '{"systemMessage":%s,"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":%s}}\n' \
                "$pass_note" "$pass_note"
        fi
    fi
    exit 0
}

pr_gate() {
    gate_raw=$(json_string_field command)
    # Every Bash call passes here, and each spawn costs tens of milliseconds on
    # Windows. A command with no `gh` or `review` in any case, and no quote,
    # escape, `$`, backtick, brace or glob that could build either, runs none
    # of them. A JSON quote is `\"`, so it counts as an escape here.
    case "$gate_raw" in
        *[gG][hH]* | *[rR][eE][vV][iI][eE][wW]* | *[\\\'\$\`{?*[]*) ;;
        *) exit 0 ;;
    esac
    command=$(json_command "$gate_raw")
    gate_lines=$(shell_statements "$command")

    # The review's record is written by the review-verdict hook, or under
    # Codex by the user; an agent writing it would be grading its own work.
    writes_marker "$gate_lines" &&
        deny "\"\`$IMPLEMENT_MARKER\` records an independent review: the review-verdict hook writes it from the reviewer's verdict line, and under Codex the user runs the recording command outside the agent. An agent never writes it.\""

    # `gh api` reaches the endpoints behind `gh pr merge` and `gh pr ready`
    # with none of the flags this gate reads, so it never carries them.
    api_ships_pr "$gate_lines" &&
        deny '"`gh api` does not merge, enqueue or un-draft a PR past this gate; use `gh pr merge` or `gh pr ready <number>`, which it checks."'

    # Recognition is not proof. Text the splitter did not read as a gated
    # statement, flattened (quotes, escapes, `$`, braces and commas removed,
    # lower-cased), still spelling a gated call or a marker write means the
    # gate cannot tell what runs, and unknown is not a pass: ask. Statements
    # it already judged, and `git` statements with their heredocs, whose text
    # is a commit message, are not rescanned. Beside a recognised gated
    # statement it is a refusal, since the two cannot share one check.
    gate_unknown=
    unknown_gated "$gate_lines" && gate_unknown=1

    # Which of the three the command is, because they are gated differently:
    # `create` opens the PR that carries the findings, while `ready` and
    # `merge` are the two ways work leaves the branch.
    #
    # Tested most-restrictive first, and that order is the rule rather than a
    # preference. A line may run more than one of them — `gh pr create --draft
    # && gh pr ready` is the natural way to end phase one and open phase two —
    # and matching `create` first would hand that line the draft exemption and
    # let the `gh pr ready` beside it through with no review at all.
    if has_statement "$gate_lines" "gh pr merge"; then
        gate_action=merge
    elif has_statement "$gate_lines" "gh pr ready"; then
        gate_action=ready
    elif has_statement "$gate_lines" "gh pr create"; then
        gate_action=create
    elif [ -n "$gate_unknown" ]; then
        ask "\"pr-gate cannot tell whether this command runs \`gh pr merge|ready|create\`, a \`gh api\` merge or un-draft, or writes \`$IMPLEMENT_MARKER\`. Run a gated command as its own plain statement; approve only if the text merely mentions one.\""
    else
        exit 0
    fi
    [ -z "$gate_unknown" ] ||
        deny '"Run the gated command alone: something beside it spells another gated call or a marker write the gate cannot read, and the two cannot share one check."'

    # Phase one ends at a draft PR, and a draft ships nothing: it cannot be
    # merged, and `gh pr ready` below is where the review is wanted.
    #
    # `--draft=false` is spelled out rather than left to the match. It is the
    # one spelling that contains the flag and means the opposite of it, and a
    # gate that reads it as a draft opens an ungated real PR.
    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    # An `implement` branch owes its one code-review before any PR opens,
    # draft included. Green CI and the user's own merge are what protect
    # `main` past it. Against `main`, or a feat/* or feature/* branch recorded
    # with no parent issue; a /feature child keeps its own authorization and
    # identity checks in the full gate.
    implement_base=$(review_base "$dir" 2>/dev/null) || implement_base=

    # Refused before any marker read or network call: the hook runs under a
    # 10 s timeout, and a hook that times out lets the command through.
    [ "$gate_action" = merge ] && [ "$implement_base" = "origin/$MAIN_BRANCH" ] &&
        deny '"Merge to main is reserved exclusively for the user; do not enable auto-merge or enqueue it."'

    if [ "$(declared_tier "$dir")" = "$IMPLEMENT_TIER" ] &&
        { [ "$implement_base" = "origin/$MAIN_BRANCH" ] ||
            sh "$(dirname "$0")/mission_base.sh" implement-target "$dir" >/dev/null 2>&1; }; then
        implement_target=${implement_base#origin/}
        # `gh pr create` defaults to `main`, which would carry the target's
        # whole stack into a PR nobody meant to open. Read from the unquoted
        # words, one value exactly: `gh` takes the last of several, and a title
        # may name the flag. Whichever verb ranks highest on the line.
        if [ "$implement_target" != "$MAIN_BRANCH" ] &&
            has_statement "$gate_lines" "gh pr create"; then
            implement_named=$(unquoted_arguments "$(gh_statement "$gate_lines" "gh pr create")" |
                sed 's/\\[nrt]/ /g' |
                tr -s ' \t\r\n\\' '\n' |
                awk 'named { print; named = 0; next }
                    /^(--base|-B)$/ { named = 1; next }
                    /^--base=/ { sub(/^--base=/, ""); print; next }
                    /^-B./ { sub(/^-B/, ""); print }')
            [ "$implement_named" = "$implement_target" ] ||
                deny "\"This branch targets $implement_target: run \`gh pr create --base $implement_target\`, once and unquoted, with the text in \`--body-file\`.\""
        fi
        implement_key=$(review_key "$dir")
        if [ -z "$implement_key" ]; then
            [ "$implement_target" = "$MAIN_BRANCH" ] ||
                deny '"Review identity against the target cannot be computed; no fallback approval is available."'
            implement_key=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
        fi
        require_marker "$dir" "$implement_key" "$IMPLEMENT_MARKER" \
            "an implement branch opens its PR only after code-review" \
            "Run the code-review skill over \`git diff $implement_base...HEAD\` and fix its valid findings"
        if [ "$gate_action" = merge ]; then
            # The trader may grant agent merges into a named non-main target by
            # listing it, one branch per line, in the common git dir's
            # `agent-merge-grant`. `main` is never grantable. A granted merge
            # still needs every PR check passing or skipped.
            implement_grant="$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/agent-merge-grant"
            if [ "$implement_target" != "$MAIN_BRANCH" ] && [ -f "$implement_grant" ] &&
                grep -qxF "$implement_target" "$implement_grant"; then
                implement_merge=$(gh_statement "$gate_lines" "gh pr merge")
                implement_pr=$(pr_number "$implement_merge")
                implement_head=$(git -C "$dir" rev-parse HEAD)
                # The grant covers this reviewed branch's own PR into the
                # target, in the head-pinned form only: no other PR, no
                # --admin, --auto or alternate repository.
                [ "$implement_merge" = "$(bare_statement "$command")" ] &&
                    case "$implement_merge" in
                        "gh pr merge $implement_pr --merge --match-head-commit $implement_head" | \
                            "gh pr merge $implement_pr --squash --match-head-commit $implement_head") ;;
                        *) false ;;
                    esac ||
                    deny '"Granted merges run alone, as `gh pr merge <number> --merge|--squash --match-head-commit <HEAD>`."'
                sh "$(dirname "$0")/mission_base.sh" check-pr "$dir" "$implement_pr" ready >/dev/null 2>&1 ||
                    deny "\"Name the PR whose base is $implement_target and whose head is this reviewed branch, after a fetch.\""
                implement_checks=$(cd "$dir" && gh pr checks "$implement_pr" --json bucket --jq '.[].bucket' 2>/dev/null) ||
                    deny '"Cannot read the PR checks for the granted merge."'
                [ -n "$implement_checks" ] && [ -z "$(printf '%s\n' "$implement_checks" | grep -v -e '^pass$' -e '^skipping$')" ] ||
                    deny '"Granted merge waits for every PR check to pass."'
                exit 0
            fi
            deny "\"Merge to $implement_target is reserved exclusively for the user; do not enable auto-merge or enqueue it.\""
        fi
        # A reused worktree may hold a PR into another base, and this review
        # covered only the diff against the target.
        if [ "$gate_action" = ready ] && [ "$implement_target" != "$MAIN_BRANCH" ]; then
            implement_pr=$(pr_number "$(gh_statement "$gate_lines" "gh pr ready")")
            sh "$(dirname "$0")/mission_base.sh" check-pr "$dir" "$implement_pr" ready >/dev/null 2>&1 ||
                deny "\"Name one PR number whose base is $implement_target and whose head is this reviewed branch, after a fetch.\""
        fi
        pass_pr_gate "$dir"
    fi

    if [ "$gate_action" = create ]; then
        gate_statement=$(gh_statement "$gate_lines" "gh pr create")
        if draft_flag "$gate_statement"; then
            pass_pr_gate "$dir"
        fi
    fi

    gate_base=$(review_base "$dir") || deny '"Review base is invalid or unavailable; reconcile the branch-bound mission-base record (campaign/* bases are retired: retarget to main, feat/* or feature/*)."'
    key=$(review_key "$dir")
    if [ -z "$key" ]; then
        [ "$gate_base" = "origin/$MAIN_BRANCH" ] || deny '"Review identity against the base cannot be computed; no fallback approval is available."'
        # The branch's own change cannot be identified - no origin/<main>, an
        # unrelated history. Fall back to the commit, which is what this gate
        # keyed on before diffs. That is strictly stricter than failing open,
        # and a checkout without origin/<main> is outside this workflow anyway:
        # every review in it measures against that ref.
        key=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
    fi
    [ -n "$key" ] || exit 0

    # Every branch owes what an implement branch owes: the code-review at this
    # exact change; full CI and the merge rules below still apply. A /feature
    # child integrates under its persisted grant.
    require_marker "$dir" "$key" "$IMPLEMENT_MARKER" \
        "no branch ships without a code-review" \
        "Run the code-review skill over \`git diff $gate_base...HEAD\` and fix its valid findings"
    [ "$gate_action" = create ] && pass_pr_gate "$dir"
    gate_pr=$(pr_number "$(gh_statement "$gate_lines" "gh pr $gate_action")")
    [ -n "$gate_pr" ] || deny '"Name the PR number explicitly."'

    # Draft pushes run only `gate` and the fast job, so readiness is where
    # full CI is owed: every FULL_CI_CHECKS verdict at the exact reviewed HEAD
    # (or, for a prose-only head, `gate` with its `rust-skip`), read from the
    # sibling script that owns the list.
    full_ci_script="$(dirname "$0")/full_ci.sh"
    [ -f "$full_ci_script" ] ||
        ask '"Full CI at the exact head cannot be verified because full_ci.sh is not beside the gate; the fast draft job is not readiness evidence."'
    full_ci_reason=$(sh "$full_ci_script" verify "$dir" 2>&1 >/dev/null)
    full_ci_status=$?
    if [ "$full_ci_status" -eq 2 ]; then
        ask "\"Full CI at the exact head could not be read from GitHub; unknown is not green. $full_ci_reason\""
    elif [ "$full_ci_status" -ne 0 ]; then
        deny "\"Full final-head CI gates \`gh pr $gate_action\`: $full_ci_reason The draft's fast job is a signal, not the verdict. Run full CI on this head with \`gh pr edit $gate_pr --add-label full-ci\`, wait for every full-CI job to pass, then retry.\""
    fi

    if [ "$gate_action" = merge ]; then
        # Only this explicit, head-pinned form is supported, bare or behind the
        # single `cd <worktree> &&` that reaches the task worktree. This also
        # rejects --admin, --auto, alternate repositories and merge-queue
        # shortcuts.
        gate_merge=$(gh_statement "$gate_lines" "gh pr merge" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
        gate_whole=$(bare_statement "$command")
        [ "$gate_merge" = "$gate_whole" ] || deny '"Run one merge command from the task worktree; compound commands cannot share an authorization check."'
        gate_head=$(git -C "$dir" rev-parse HEAD)
        case "$gate_merge" in
            "gh pr merge $gate_pr --merge --match-head-commit $gate_head"|"gh pr merge $gate_pr --squash --match-head-commit $gate_head") ;;
            *) deny '"Agent merges require an explicit PR, --merge or --squash, and --match-head-commit with the reviewed HEAD; no auto-merge, admin override or alternate repository."' ;;
        esac
    fi
    if [ "$gate_base" != "origin/$MAIN_BRANCH" ]; then
        if [ "$gate_action" = ready ]; then
            gate_ready=$(bare_statement "$command")
            [ "$gate_ready" = "gh pr ready $gate_pr" ] || deny '"Run one explicit ready command in the task worktree, without repository overrides or compound commands."'
        fi
        if ! sh "$(dirname "$0")/mission_base.sh" check-pr "$dir" "$gate_pr" "$gate_action" >/dev/null 2>&1; then
            deny '"PR identity, authorization or CI could not be verified. Reconcile its exact base/head, persisted merge grant and green checks before continuing."'
        fi
    fi

    pass_pr_gate "$dir"
}

# --- commit-reminder --------------------------------------------------------

commit_reminder() {
    command=$(json_string_field command)
    case "$command" in *commit*) ;; *) exit 0 ;; esac
    command=$(json_command "$command")
    runs_command "$command" "git commit" || exit 0

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    reminder_base=$(review_base "$dir") || exit 0
    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
    [ "$branch" != "$MAIN_BRANCH" ] || exit 0

    ahead=$(git -C "$dir" rev-list --count "$reminder_base..HEAD" 2>/dev/null) || exit 0
    [ "${ahead:-0}" -gt 0 ] || exit 0

    if [ "$(declared_tier "$dir")" = "$IMPLEMENT_TIER" ]; then
        context "\"Branch \`$branch\` is $ahead commit(s) ahead of $reminder_base at the \`implement\` tier, so opening its PR needs a current \`$IMPLEMENT_MARKER\` from the code-review skill.\""
    fi

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of $reminder_base. A draft PR opens freely; a non-draft PR, \`gh pr ready\` and \`gh pr merge\` need a current \`$IMPLEMENT_MARKER\` from the code-review skill.\""
}

# PostToolUse on Edit|Write. Runs the repository guards over the file that was
# just written and reports what they found, so a crossed size ceiling, a
# non-English word or a codepage round-trip surfaces at the edit that caused
# it rather than at the end of a `cargo test --workspace` run minutes later.
#
# Three properties make this safe to run on every write:
#
#   It never blocks. PostToolUse cannot deny — the edit has already landed —
#   and that is the right shape here. The gate stays `cargo test --workspace`;
#   this only moves the *news* earlier.
#
#   It never invokes cargo. It runs the already-built binary straight out of
#   `target/`, so it cannot contend for the build lock with a `cargo build`
#   the agent is running, and cannot silently trigger a four-minute compile of
#   its own. No binary means no output: the guards still run in the suite.
#
#   It reads one file. `--file` checks that path against the baseline instead
#   of walking the repo, which is milliseconds rather than the seconds a full
#   scan costs.
guard_watch_one() {
    file=$(absolute_edit_path "$1")
    # No extension filter here on purpose. One lived here and was a third
    # hand-kept copy of a list the two Rust guards already own; adding an
    # extension there while forgetting it here would have left the suite
    # seeing a file the edit-time hook silently did not — an all-clear that
    # reads exactly like a clean file. Each guard's `check_file` already
    # returns nothing for a path it does not read, so the filter bought a
    # process spawn and cost a drift.
    # An absolute path or nothing. A relative `file_path` would make `dirname`
    # answer `.`, and both git queries below would then resolve against this
    # hook process's own working directory — the main checkout — so the guards
    # would run over a same-named file in a tree the author is not editing
    # while the file they did edit went unchecked.
    case "$file" in
        /* | ?:/*) ;;
        *) return 0 ;;
    esac

    dir=$(dirname "$file")
    [ -d "$dir" ] || return 0

    # Both answers from one process. `--show-toplevel` and `--show-prefix`
    # were two spawns for what one call prints on two lines, and on Windows a
    # spawn costs tens of milliseconds — against a binary that answers in 27.
    # This mode fires on every write in the session, so the shell plumbing had
    # become the dominant cost of the thing built to be cheap.
    location=$(git -C "$dir" rev-parse --show-toplevel --show-prefix 2>/dev/null) || return 0
    root=$(normalize_path "$(printf '%s\n' "$location" | sed -n '1p')")
    prefix=$(printf '%s\n' "$location" | sed -n '2p')

    # The binary the branch already built, under either name this platform
    # gives it. Absent is the ordinary case on a fresh worktree, and silence
    # is the correct answer to it.
    binary="$root/target/debug/quantick-guards"
    [ -x "$binary" ] || binary="$binary.exe"
    [ -x "$binary" ] || return 0

    # Workspace-relative, forward slashes: the spelling the baseline uses.
    # `$prefix` above comes from git rather than from subtracting the root out
    # of the absolute path, because the two are not spelled alike here — the
    # payload carries the path the way the host writes it and
    # `--show-toplevel` answers the way git does, so under Git Bash
    # `/tmp/x/src/a.rs` and `C:/Users/.../Temp/x` describe the same tree and
    # share no prefix. The subtraction silently produced no match, which reads
    # exactly like a clean file.
    relative="$prefix$(basename "$file")"

    # The binary is told which root to read, because the one compiled into it
    # is whichever worktree happened to build it.
    findings=$(QUANTICK_GUARDS_ROOT="$root" "$binary" --file "$relative" 2>&1) && return 0
    [ -n "$findings" ] || return 0

    # JSON-escape by hand, because there is no jq here. Separate `-e` scripts
    # and a `|` delimiter: a `;`-joined script with a `/` delimiter is what
    # this environment's sed rejects, and it rejects it to stderr while the
    # pipeline still exits 0 — which produced an empty message that read as a
    # clean file.
    escaped=$(printf '%s' "$findings" |
        sed -e 's|\\|\\\\|g' -e 's|"|\\"|g' |
        awk 'BEGIN { ORS = "" } { if (NR > 1) printf "\\n"; print }')
    # `$relative` gets the same treatment. It is derived from a filename, so a
    # quote in it would close the JSON string early and the harness would drop
    # the whole payload — leaving the author with the silence that reads as a
    # clean file. Escaping the findings and interpolating the path raw was the
    # same defect `require_marker` goes to some length to avoid on the deny
    # path.
    escaped_path=$(printf '%s' "$relative" | sed -e 's|\\|\\\\|g' -e 's|"|\\"|g')
    context "\"Repository guards on \`$escaped_path\`:\n$escaped\n\nThis is advisory and blocks nothing; \`cargo test --workspace\` is still the gate. Fix it now while the edit is in hand, or run \`cargo run -p quantick-guards -- --tighten\` if a baseline entry only needs lowering.\""
}

guard_watch() {
    paths=$(edit_paths)
    [ -n "$paths" ] || exit 0
    old_ifs=$IFS
    IFS='
'
    for file in $paths; do
        guard_watch_one "$file"
    done
    IFS=$old_ifs
    exit 0
}

# --- review-verdict ---------------------------------------------------------

# A background code-review hands its report back through SubagentHandback; a
# foreground fork ends on it as the subagent's last message. The main agent's
# own text is never read; a subagent could still write the line, so this is
# workflow evidence like every marker, not a boundary. The report's last
# `REVIEW-VERDICT: pass|fail <worktree> <reviewed HEAD>` counts only while that
# worktree sits at that HEAD, whatever its tier, since pr-gate asks every
# branch for the marker: `pass` writes
# `review_key`, the value pr-gate compares, so a later commit leaves it stale;
# `fail` removes the record. Anything else changes nothing.
review_verdict() {
    verdict_flat=$(printf '%s' "$input" | tr -d '\r\n')
    if [ "$(json_string_field hook_event_name)" = SubagentStop ]; then
        verdict_report=${verdict_flat#*\"last_assistant_message\"}
    elif [ "$(json_string_field tool_name)" = SubagentHandback ]; then
        verdict_report=${verdict_flat#*\"tool_input\"}
    else
        exit 0
    fi
    [ "$verdict_report" != "$verdict_flat" ] || exit 0

    # JSON spells C:\src\x as C:\\src\\x: one slash first, so a path segment
    # starting with n never reads as a \n line break.
    verdict=$(printf '%s' "$verdict_report" |
        sed 's/\\\\/\//g; s/\\[nrt]/\n/g' |
        sed -n 's/^.*REVIEW-VERDICT:[[:space:]*_]*\(pass\|fail\)[*_]*[[:space:]]\{1,\}["`]*\([^"`[:space:]]\{1,\}\)["`]*[[:space:]]\{1,\}\([0-9a-fA-F]\{7,40\}\).*$/\1 \2 \3/p' |
        tail -n 1)
    [ -n "$verdict" ] || exit 0
    set -f
    # shellcheck disable=SC2086 # three words, none of them globbed
    set -- $verdict
    set +f
    verdict_reviewed=$(printf '%s' "$3" | tr 'A-F' 'a-f')
    dir=$2
    [ -d "$dir" ] || exit 0
    case "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" in
        "$verdict_reviewed"*) ;;
        *) exit 0 ;;
    esac
    verdict_file=$(marker_path "$dir" "$IMPLEMENT_MARKER") || exit 0
    if [ "$1" = fail ]; then
        rm -f "$verdict_file"
        exit 0
    fi
    verdict_key=$(review_key "$dir")
    [ -n "$verdict_key" ] || exit 0
    printf '%s\n' "$verdict_key" > "$verdict_file"
    exit 0
}

# A machine whose cargo has no `build.jobs` cap runs every invocation at one
# job per core, so N parallel agents ask for N times the cores and the RAM to
# match. The fix is per machine and outside the repository, which is why it
# reaches a session as news at its start rather than as a rule it must
# remember: a gate cannot do it, and setup changes user-level config only with
# the user's consent. Silent once a cap exists in the environment or in either
# file cargo reads from CARGO_HOME.
machine_check() {
    [ -z "${CARGO_BUILD_JOBS:-}" ] || exit 0
    machine_cargo_home=${CARGO_HOME:-${HOME:-}/.cargo}
    for machine_config in "$machine_cargo_home/config.toml" "$machine_cargo_home/config"; do
        [ -f "$machine_config" ] || continue
        if grep -Eq '^[[:space:]]*jobs[[:space:]]*=' "$machine_config" 2>/dev/null; then
            exit 0
        fi
    done
    printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":%s}}\n' \
        '"This machine has no cargo build.jobs cap, so every cargo command takes one job per core and parallel agents oversubscribe CPU and RAM. Before your first build, tell the user and, only with their consent, apply docs/dev-machine.md; on Windows that is: powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev_machine/setup.ps1"'
    exit 0
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    guard-watch) guard_watch ;;
    review-verdict) review_verdict ;;
    machine-check) machine_check ;;
    *) exit 0 ;;
esac
