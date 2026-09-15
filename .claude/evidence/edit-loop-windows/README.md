# C2 implementation evidence bundle

This bundle records local implementation and verification for issue
[#477](https://github.com/milocaetano/quantick/issues/477), not a score,
accepted calibration or completed mission. Read the
[implementation report](../../../docs/workflow/evidence/edit-loop-windows.md)
and [mission archive](../../GOAL-archive-edit-loop-windows.md) first.

No actual edit-loop timing series, numeric fixed budgets, hosted Linux safety
execution, final-head CI or final independent review is claimed. The first
hosted calibration is expected to fail the explicit uncalibrated gate after
retaining a complete valid series. It is never counted as a passing check.

## Record format and provenance

[manifest.json](manifest.json) lists every copied record's path, SHA256 and
original source name/hash. Each record is a JSON envelope containing the
complete decoded original text under `source_text`. Original CRLF, indentation
and trailing spaces remain inside the escaped string, avoiding a whitespace
rewrite of raw results. Source hashes identify original private bytes; explicit encoding/BOM metadata
allows reconstruction (five legacy PowerShell logs are UTF-16LE with BOM).
Envelope hashes identify the committed representation. No full transcript,
implementing-model reasoning or unrelated session extract is included.

Source records preserve the preflight request/map, original findings,
independent follow-up and root conformance. Those verdicts are limited to
their declared preflight scope, not reused as final reviews. The historical
pre-freeze feedback's PF5-in-progress status is superseded by the retained
30-test receipt and current source, not rewritten.

History records retain baseline prechecks, every named development failure,
the old-base suite failure, unsuccessful causal hypothesis, bounded passing
diagnostics and the explicitly authorized step-4 retry. A narrow recovered
bootstrap Project error preserves outer exit 0 and unknown inner numerical
exit, not an invented nonzero wrapper. The capture-provenance record describes
the private evidence-edit diagnostics and recovered original tool output.

Current records bind incoming-base inspection, safe rebase with 16 identical
tooling blobs, all seven frozen files, current offline checks and every fresh
ordered-loop output chunk to code head 0ca889b9e3c3b6b60dd52002f737d17af56082d8
and tree 71c407d226dc6fc80858506d4eb1831729cbd97b. Final archival evidence is
reused only for an inspected evidence/mission-record delta; it is not renamed
as a newly executed code head.

The contaminated original `full-loop-1e76097a.log` Start-Transcript file is
explicitly excluded, as are broad session logs, transcript-extraction scripts,
executables, stores, descriptors, credentials and user configuration. Complete
scoped tool chunks, not that file, prove validation. Scope-limited recovery
does not claim a complete author-action audit.

## Finding and operation identity

C2-PF1/PF2 retain the source-map findings and reviewed revision-2 dispositions.
C2-PF3/PF4/PF5 retain process ownership, exact-SHA/input coverage and explicit
quiescence receipt findings. Their implementation-stage fixture corrections
are not formal review PASS. Operation failure count remains 1 from the
initial rejected branch prefix; formal review-repair batches remain 0.
Actual development/test/invocation errors are separately retained.

The manifest excludes itself and this explanatory index, avoiding recursive
self-hashing. The Git commit binds those two generated files. Final archive
guard/hygiene/link/manifest and unchanged-input checks are retained privately
for coordinator publication against the exact handoff head.
