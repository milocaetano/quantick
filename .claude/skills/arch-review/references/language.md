# Dimension 8 — grading the language rule

Read before writing a language finding. `CLAUDE.md` owns the rule itself and its exemptions; this is how it is graded.

**`CLAUDE.md` owns this rule** — what is in scope, and the three exemptions
where the foreign text *is* the data. Read it there; this dimension does not
restate it, because a scope list kept in two places is dimension 3's own
"second copy is the finding" applied to prose, and it drifts on the first edit.
What lives here is how to grade it.

Grade only what the diff **authors**. Lines that predate the rule are
grandfathered, and a diff that relocates, reindents or deletes one is not
writing it — a cleanup that translates an old comment must not earn a finding
for the Portuguese it is removing. The known pre-existing debt, so nobody
re-litigates it: `docs/ux/drawing-tools-ux-spec.html` (a full spec, ~46 lines),
`heatmap-design-ref/`, the tracked `.claude/GOAL-archive-*.md`, and two doc
comments in `app.rs` / `fib.rs` that quote the trader and are exempt anyway.
Translating any of them is welcome as its own change; this rule never demands
it.

Severity: a line the diff authors in another language is a **Blocker**. Not
because the line is wrong — usually it is the clearest sentence in the file.
Because the moment two languages are tolerated the boundary is never drawn
again: the next reader is locked out of half the codebase, every grep runs
twice, and a contributor who reads neither language has no way in.

**The mechanical half is a test, not a paste.**
`crates/guards/src/language.rs` runs in `cargo test --workspace` and in
CI, holds the allowlist for the debt above, and fails on a new accented run or
Portuguese keyword in `.rs`, `.pine` and `docs/`. That is the repo's own
pattern for a rule the compiler cannot see (`crates/guards/src/encoding.rs`,
`fmath_guard.rs`), and it is why this dimension does not ship a grep recipe:
one was drafted, and it silently missed every accented uppercase word (GNU
grep's `-i` does not case-fold multi-byte characters here) and every
identifier (`_` is a word character, so a snake_case hump offers `-w` no
boundary). A check that comes back clean for the wrong reason is worse than
no check at all.

So the reviewer's job in this dimension is the part the guard cannot do:

- What the guard does not scan — the **branch name, the commit messages and
  the PR title and body**, none of which appear in a file. Read them:
  `git log --format='%s%n%b' origin/main..HEAD` and
  `git rev-parse --abbrev-ref HEAD`.
- Foreign prose the guard's keyword list does not contain — a sentence built
  entirely from words it never learned, or a language it was never taught.
- Whether an exemption is honestly claimed: the string inside a fixture may be
  foreign, the comment above it may not.

Report the guard's verdict and your own separately. "`quantick-guards` language passes"
is not the same claim as "I read the prose".

