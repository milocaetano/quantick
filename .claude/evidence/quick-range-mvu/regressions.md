# Issue447 regression dispositions

Baseline is the exact locally merged campaign9ff/maina664 input with only
characterization tests, not the current candidate. Its retained app test
executable SHA256 is
C076FF05E14F8629B3850242426F80F6A743E7A8AD972A59E68A51133AD84ECB.

| Finding | Baseline executable result | Candidate disposition |
| --- | --- | --- |
| Rewrite/prepend/rebin invalidates selected slots | Expected failure in quick_range_rewrite_never_offers_a_profile_over_stale_slots | Revision reconciliation makes the range stale before paint or conversion; disabled chrome explains replacement is needed |
| Equal opening timestamps select the wrong bars | Expected failure in quick_range_conversion_keeps_slots_with_identical_open_times: [199,199] instead of [80,160] | Existing v2 receives an optional exact chart reference; slot, timestamp, pane, revision and layout are validated together |
| Persistent selection leaves temporary chrome above it | Expected failure in quick_range_yields_chrome_when_a_persistent_drawing_is_selected | Persistent selection event reconciles before floating action chrome |
| Live tape lane starts a historical range | Original candidate test and direct owner-state diagnostic both PASS at9ff57501; source protection predates quick-range | Not a reproduced defect: existing history-only eligibility is preserved in the headless owner; no fourth repair is claimed |

The first three failed before implementation (session96733); the shipped
Fibonacci range characterization passed. The fourth test was added later:
its initial candidate failure was an incomplete fixture with no visible lane,
not evidence that the old product failed. That fixture was repaired, then
passed (session69772). It must not be presented as failing-before evidence.
The subsequent baseline run disproved the fourth finding's stated premise:
the price band was already history-only before quick-range existed. See
tape-lane-baseline.md for exact commands, hashes, owner-state observations and
ancestor commits. #447 explicitly permits a reasoned non-defect disposition;
the current map reconciles that original allowance rather than inventing red
evidence. Historical worktrees and binaries remain intact. Final #447 readback
is recorded on #447 in comment 5675552401 and reciprocally on #501 in
comment 5675566601. Current delivery review remains pending.

All three current actions retain their existing versions, author/admission
path, and future-coordinate support. Optional exact references do not change
legacy timestamp mode: an otherwise unused fallback position remains ignored.
The latter compatibility assertion failed in the extracted core before repair;
13 headless tests passed afterward. Current full-workspace validation remains
separate from these focused receipts.
