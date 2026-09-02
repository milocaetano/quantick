# Reproduction — step 0 ran at a level the tier never bought

Measured 2026-09-01, on the machine that filed the report, against Claude Code
`2.1.258`. The trader saw the failure once and said so: *"eu vi isso uma vez.
Pode ser específico da minha sessão."* It is not. It reproduces on demand, it
has a single mechanical cause, and the cause is in this repository's own
instructions rather than in the bundled skill.

**Verdict: reproduces, 2/2, deterministically.**

## 1. The parser takes the effort as the *first* token

The bundled `code-review` is not a file under `.claude/skills/`; it is compiled
into the CLI. Its own published argument hint, read out of the shipped binary,
is:

```
[low|medium|high|xhigh|max|ultra] [--fix] [--comment] [<pr#>|<branch>|<path>]
```

Effort first, flags second, target last. The parser behind it reads the first
non-flag token, tests it against the level list, and branches:

- first token **is** a level → that level is `explicit`, and the target is
  everything *after* it;
- first token is **not** a level → `explicit` is undefined, and **the entire
  argument string becomes the target**, effort word and all.

There is no third branch. A first token that is not a level is not an error the
skill reports — it is a target.

So `arch-review`'s documented form, `args: "<target> <effort>"`, failed twice
over on one call. It lost the level, *and* it handed the review a target of the
shape `my-branch medium`. The second half is what the skill's own *Check the
scope it comes back with* paragraph had been catching for months, without ever
naming what caused it.

## 2. The level it falls back to is global, not per session

With no explicit level, the skill reads `codeReviewLastEffort` from
`~/.claude.json` — a single value shared by every session and every project on
the machine, persisted only when a human *types* a level at the prompt. Read
directly from that file on this machine at the time of the report:

```
codeReviewLastEffort = xhigh
```

Exactly the level the trader saw run. Nothing about the failure was specific to
that session; any session on this machine, invoking the documented form, would
have got `xhigh`.

## 3. Two live invocations, differing only in token order

Same target, same session, minutes apart. The only variable is which word comes
first.

| | Run A — documented order | Run B — effort first |
| --- | --- | --- |
| args | `crates/guards/src/lib.rs low` | `low crates/guards/src/lib.rs` |
| opening line of the report | **"Reusing your last effort level, xhigh — type a level (for example `/code-review high`) to change it."** | *(no notice at all)* |
| where it ran | forked to a background agent | inline |
| wall clock | 405 s | returned immediately |
| subagent tokens | **111,465** | none — no subagent |
| what came back | 15 findings across the whole guards crate, its callers and its tests | correctly reported that the target has no reviewable diff |

Run A asked for `low` and was given `xhigh`, on a 129-line file with no changes
in any diff. Run B asked for `low` and got `low`.

One honest note about run A: the report says *"I reviewed
`crates/guards/src/lib.rs` as the target"*, so the agent recovered the path by
judgement from the mangled string it was handed. That recovery is the agent
being sensible, not the parser working — and it is precisely why the defect
survived so long. The target usually still resolves, so the only symptom left
to notice is the bill.

## 4. What this repository still taught

Grep over tracked files for the wrong argument order, after the fix:

```
$ grep -rn "code-review <PR>\|args: \"<target>" --include=*.md .
./.claude/GOAL-archive-control-scene.md:46:  ... (step 0 `code-review <PR> high`) ...
./.claude/GOAL.md:145:> - arch-review/SKILL.md manda invocar `Skill(code-review), args: "<target> <effort>"`
```

Two live instructions carried the wrong order and are fixed on this branch:
`.claude/skills/arch-review/SKILL.md` (the invocation block) and
`docs/control-plane/roadmap.md` (`code-review <PR> high` → `code-review high
<PR>`). The two hits above are deliberately left alone — the first is an
archived goal file and the second is the trader's own quoted bug report inside
this branch's goal file. Both are records of what happened, not instructions to
repeat it.

## 5. Incidental, and not this branch's work

Run A's 15 findings are real and are about `crates/guards/`, which this branch
does not touch. They are recorded here as the by-product of a reproduction, not
adopted as scope: acting on them would be exactly the widening this mission's
ledger exists to prevent. They are worth an issue of their own.
