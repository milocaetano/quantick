# Campaign recovery helper

`architecture-a-coordinator-v2.py` is the maintained copy of the coordinator
adopted by campaign #330. It journals operations, preserves history and publishes
bounded, hash-verified checkpoint partitions. It does not schedule tasks,
authorize business actions or provide an ownership takeover API.

The initial source and all 53 original tests are retained byte-for-byte from
their [adopted source publication](https://github.com/milocaetano/quantick/issues/350#issuecomment-5596003122)
and [original test publication](https://github.com/milocaetano/quantick/issues/350#issuecomment-5595674363):

| File | Initial SHA256 |
| --- | --- |
| `architecture-a-coordinator-v2.py` | `ad028583da5951656f89e84ab24c492913ada31c9c91b2746db9a057dbc685e4` |
| `test-architecture-a-coordinator-v2.py` | `2c9a432068d02891f6245621f28daed50ca3731ba06d86e53421151e60c8d3b8` |

The historical constants/docstring are also retained. Operational callers must
explicitly configure `ROOT` to their own cache directory and `WRITER` to their
actual coordinator identity before calling `activate`. Import has no I/O and
does not activate the module. Load the repository file with
`importlib.util.spec_from_file_location`; there is no command-line wrapper.
Record the exact reviewed file/hash, reviewed integration and legitimate
ownership in the campaign before adopting a new implementation revision.

`activate(receipt)` requires the recorded `adoption_checkpoint`, full
`reviewed_sync_merge` and `source_main_sha`, and
`format: quantick-snapshot-parts:v1`. These values record prior adoption;
passing them is not a grant of authority. The default `GhTransport` uses the
authenticated `gh` CLI for this repository. Follow the
[state contract](../../docs/campaign/state.md) for the actual user grant,
single-writer ownership, lease recovery and business-effect readback.

| Entry point | Result and caller responsibility |
| --- | --- |
| `read_checkpoint(url, transport)` | Returns complete reconstructed state or refuses missing/corrupt parts. A manifest alone is not state. |
| `Publisher.boundary(url, campaign)` | Reads the current boundary and subsequent journals, rejects conflicts/expired ownership, and returns folded records. |
| `intent(state, key, target, mutation, expected)` | Publishes stable pending intent, returns its record/URL. Persist this before the business effect. |
| `result(pending, status, evidence)` | Publishes the same key/attempt/identity with actual result evidence. The caller must compare the external effect with expected readback first. |
| `checkpoint(state, release=False)` | Folds observed results and preserved history into a complete checkpoint. On return, `state['previous']` points to the new public boundary. |

For recovery, read the public boundary and later journals, inspect the actual
effect by its original operation key, and reuse the observed attempt. Missing
evidence is unresolved. An expired/foreign owner, contradictory journal or
unknown legacy attempt requires explicit legitimate reconciliation; do not
change identities or relax checks. The helper preserves a frozen local
checkpoint payload across uncertain publication; reconcile its exact public
result before changing that request or starting another operation.

Run the independent offline suites from the repository root:

```sh
python tools/campaign/test-architecture-a-coordinator-v2.py
python tools/campaign/test_campaign_recovery.py
python .claude/hooks/review_progress_test.py
ruff check --select F tools/campaign/
sh .claude/hooks/guardrails_test.sh
```

The existing guardrail CI step runs both coordinator suites and the separate
PR-review-progress suite; the existing Python lint step already covers all of
`tools/`. No network, credentials, GUI, real campaign cache or third-party Python
package is needed by the tests.

The new composite scenario launches two real Python processes with separate
empty caches. Only fake remote data and an explicit clock survive the first
process's accepted-but-lost effect response. The second process uses the actual
helper to reconstruct partitions, reconcile the original attempt and publish
an independently checked dependency/evidence audit. The fake remote deliberately
does not deduplicate business calls, so a duplicate would be observable.
Refusals cover damaged partitions, conflicting pending identity, foreign owner,
expired lease and changed business readback. A changed upstream CI result stops
the audit after recovery. Actual `GhTransport.comments_since` is tested through
raw fake CLI JSON across three five-comment pages, including a missing boundary.

This proves an offline restart of the same logical writer. The test driver
performs the business audit and expected-readback comparison; neither is an
invented helper scheduler or authorization check. Original tests separately
prove refusal to delete/renumber preserved unknown history in a proposed
checkpoint; a legacy checkpoint with an optional history field is not declared
invalid merely for lacking that field.

[Q12](https://github.com/milocaetano/quantick/issues/353) owns actual adoption
of the reviewed repository file, the live URL-only replacement exercise and
integrated independent assessment. Q11's tests do not prove those outcomes or
award an AD5 point. See the [evidence record](../../docs/quality/campaign-recovery-evidence.md).
