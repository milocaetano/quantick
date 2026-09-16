# Safety and frozen inputs

Working-file hashes were read with git hash-object (no writes). Every result
matches the initial seven-path manifest retained in the previous sync mission.

+docs/quality/outside-score-rubric.md: 0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76 — MATCH
tools/outside_score/measure.py: 494ff11e6231f513d500fe935fe27fe3c2d2e2e5 — MATCH
.claude/skills/outside-score/SKILL.md: 81d101d7e0fad73f1390977be7671545da46ec51 — MATCH
.agents/skills/outside-score/SKILL.md: 45cc224fa29f2881601e03fe462e242bd50e8521 — MATCH
.claude/skills/quantick-score/SKILL.md: a0d008fbab2565fcfa1fc1b6894b075bf47f3526 — MATCH
.agents/skills/quantick-score/SKILL.md: 5489246f5748b8b8abc80696d88cb75e331d6179 — MATCH
docs/quality/quantick-score-rubric.md: c7fd2af3aa108abdff7b3083b4321fd941d8a813 — MATCH

The precommit receipt above used MERGE_HEAD a6644bc6 and HEAD 9ff57501.
The combined source is now committed at
1b4bad7ebb257d0c0a016db6994338592b367bda, with those two exact parents.
Both ancestry checks pass. Subsequent evidence-only updates do not alter
either ancestry or any of the seven frozen inputs.

Read-only git diff against9ff found no source changes under crates/feed/src,
crates/feed-mt5/src, crates/control-host/src, app/control/gateway or
app/control/contract.rs. Existing feed continuity, admission and retry behavior
is not replaced by this extraction. Full workspace regression proof remains
required; source equality is not a test result.
