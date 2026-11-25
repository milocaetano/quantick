# Preparing a machine for parallel agents

Run once per machine, and again whenever it feels starved:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev_machine/setup.ps1
```

Add `-Check` to change nothing, and the switches below to opt into more.
Everything it changes lives outside the repository, in user-level config, so
it is never part of a pull request and is undone by editing the same files.

## Why

Cargo starts one job per logical core, and that limit belongs to one cargo
process. Nothing coordinates two of them. One agent building on a 16-thread
machine uses the whole machine, which is the intended trade. Four agents
building at once ask for 64 rustc and link processes on 16 threads. The
largest of those are `quantick-app` test builds near 2 GB each. The machine
pages, the chart stutters, and every build gets slower than it would have been
in a queue.

The crate count is not what drives this. The process ceiling is
*concurrent cargo invocations × jobs each*. Merging crates would raise the
peak memory of the biggest rustc and leave that product unchanged.

## What the script does

| Step | Default | Opt-in switch | Effect |
| --- | --- | --- | --- |
| Cargo jobs cap | applies | `-Jobs <n>` to pick the number | Writes `[build] jobs = <half the logical cores, at least 2>` to `$CARGO_HOME/config.toml` when no cap exists. It never touches an existing `jobs` key, and it leaves a file it cannot extend safely (dotted `build.*` keys, an inline table) to you rather than write a second `[build]` table. |
| Merged worktrees | reports | `-PruneMerged` | Removes clean worktrees whose branch is merged into `origin/main`, then deletes their branches. It keeps a worktree that has uncommitted changes or is still held by a process. A worktree just cut from `origin/main` is not "merged"; a clean branch just cut from an integration branch that is already merged would be, so close sessions before pruning. |
| Agent priority | reports | `-LowerAgentPriority` | Sets running `claude` processes to BelowNormal. The cargo they start afterwards inherits it, so the chart and the editor stay responsive. Use BelowNormal, not Idle: Idle starves builds. |
| rust-analyzer plugin | reports | `-DisableRustAnalyzerPlugin` | Turns off the `rust-analyzer-lsp` plugin in `~/.claude/settings.json`. Otherwise every agent session that touches the LSP tool can start its own instance, typically 2–6 GB plus a check on save, outside any cap. Takes effect in new sessions. |
| Defender | reports | none | Says whether real-time protection is on. If scanning is the cost, keep it on and put the worktrees on a Dev Drive rather than excluding folders. |

On another OS the jobs cap is the step that matters:

```toml
# ~/.cargo/config.toml
[build]
jobs = 8   # about half the logical cores
```

## How agents learn about it

The `machine-check` hook (`.claude/hooks/guardrails.sh`, SessionStart on
`startup`, wired for Claude Code and Codex) is
silent once a cap exists, either in `CARGO_BUILD_JOBS` or as a `jobs` key in
cargo's config. Without one, it tells the session to raise this with the user
and to run the script only with their consent. It never blocks.

## What it deliberately does not do

- **Share one `CARGO_TARGET_DIR` across worktrees.** Workspace crates hash to
  the same file names in every worktree, so concurrent builds overwrite each
  other's artifacts and rebuild in turn. They also serialise on one build lock.
- **Set global `CARGO_*` or `RUST*` variables, sccache or a linker.**
  `tools/edit_loop` rejects inherited build overrides, because a measurement
  taken under one would not be comparable. `build.jobs` in the config file is
  the one knob it tolerates, because it passes its own `CARGO_BUILD_JOBS`.
- **Change the `dev` profile.** The trader debugs that profile. CI trims
  debuginfo through its own environment instead.
