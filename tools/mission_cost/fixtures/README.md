# Synthetic transcript fixture

Every line under `transcripts/` was written by hand for these tests. Nothing
here was copied from a real session transcript, and nothing here is the
trader's. The campaign's constraint is that the harness never reads transcript
content, so the fixture's job is to make a violation fail loudly rather than
pass quietly.

That is why several lines carry deliberate bait — `message.content` strings,
`cwd`, `gitBranch`, `sessionId`, and `usage` keys the method does not count.
Each bait string contains `MUST-NOT-APPEAR` or `SHOULD-NOT-BE-READ`, and
`test_transcripts.py` asserts that no such string survives into a record or a
rendered report. A reader widened to take one more field fails there.

The layout mirrors the real one: `<session>.jsonl` for a main thread and
`<session>/subagents/agent-*.jsonl` beside it.

| Session | Shape | What it exercises |
| --- | --- | --- |
| `aaaaaaaa-…0001` | main plus two subagents | the main/subagent split, the 300-second idle bound, a line with no `usage` |
| `bbbbbbbb-…0002` | main only | a session overlapping two mission windows: the shared bucket |
| `cccccccc-…0003` | subagents only, no root file | `partial_main_thread` |
| `dddddddd-…0004` | main only, far outside every window | the unassigned bucket |
| `eeeeeeee-…0005` | main only, far outside every window | a declared session overriding the window rule |

`missions.json` beside them is the registry those cases are read against.
