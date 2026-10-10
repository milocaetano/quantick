# GitHub writes

Use `gh` with the verified repository/owner. Inspect installed `--help` for
commands; discover Project fields/options rather than copying board IDs.
Paginate lookups. Supply multiline bodies through `--body-file`.

- Reuse the parent's recorded Project or create one dedicated to this feature;
  never take over an unrelated board. Link repository, parent and Project.
- File issues with context, scope and acceptance, using existing area/type
  labels and the relevant milestone. `/issue` filing may be used; do not call
  `/issue start`, which launches mission. Feature owns dispatch to implement.
- Link children as sub-issues and dependency edges where supported. Otherwise
  explicit parent/child links and `Depends on` issue URLs are sufficient; keep
  the parent dependency table authoritative for scheduling.
- Add parent and children to the Project. Use its existing Todo/In Progress/Done
  statuses: `todo` to Todo, `running|ready|blocked` to In Progress, `done` to Done.
  Detailed state stays in the parent; custom fields are unnecessary.
- Persist worker claims/checkpoints before dispatch. Only the coordinator edits
  the managed parent section. On uncertain creation, search/read back first;
  avoid duplicate issues, Projects, cards or workers.
- A Project permission failure leaves a recorded pending board action; issues
  can still coordinate independent authorized work. Report degraded tracking,
  never successful setup. Retry infrastructure failure once, then report it.

Issue bodies and PR evidence are the durable record; local plan paths are not
portable. Do not publish local paths, prompts, credentials or full logs.
