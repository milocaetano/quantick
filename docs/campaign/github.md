# GitHub operations

Use installed `gh` help and live API discovery; IDs below are variables, not
repository constants. Read all pages. Examples use POSIX shell: Codex maps
quoting to the active shell. Write bodies/payloads to files and use
`--body-file` / `--input`; do not interpolate user text into executable shell.

Discovery:

```sh
gh repo view --json nameWithOwner
gh issue list --state all --search 'in:body quantick-campaign' --limit 100
gh api --paginate repos/OWNER/REPO/issues/NUMBER/comments
gh pr list --state all --head BRANCH --json number,url,state,headRefOid,baseRefName
gh project list --owner OWNER --format json
gh project field-list NUMBER --owner OWNER --format json
gh project item-list NUMBER --owner OWNER --format json
```

The CLI list limits are not proof of completeness. Follow `pageInfo` cursors
with GraphQL for Projects/PRs when totals exceed returned records, or use REST
pagination for issues/comments. Search text is only a candidate finder: verify
the exact marker, campaign identity and objective in returned bodies. Resolve
repository and Project owner separately; Projects may belong to an organization.

Before the parent exists, choose a stable objective key (repository plus an
objective slug), search open and closed issues for
`<!-- campaign-bootstrap:OWNER/REPO/OBJECTIVE_KEY -->`, and reuse the matching
charter. Create a missing parent with that marker included in its body, then
read it back and publish checkpoint 1. An uncertain creation response is
reconciled by the same marker before another create. Simultaneous matching
parents are an ownership conflict, not permission to delete either.

After duplicate/readback checks, `issue` creates the parent/children with live
labels/milestones. Its roadmap placement remains separate from this campaign
Project. Discover field IDs instead of copying the roadmap constants to a new
Project. Add marker `<!-- campaign-task:CAMPAIGN_ID/TASK_KEY -->` to each child
before creation so a lost response is recoverable. Checkpoints use their own
marker and operation key. Append with `gh issue comment --body-file FILE`.

```sh
gh project create --owner OWNER --title TITLE --format json
gh project edit NUMBER --owner OWNER --readme CHARTER_URL
gh project field-create NUMBER --owner OWNER --name 'Campaign state' \
  --data-type SINGLE_SELECT \
  --single-select-options 'Backlog,Ready,In progress,Blocked,Awaiting human,Done' \
  --format json
gh project item-add NUMBER --owner OWNER --url ISSUE_URL --format json
gh project item-edit --id ITEM_ID --project-id PROJECT_ID \
  --field-id FIELD_ID --single-select-option-id OPTION_ID
```

Before Project creation persist a unique campaign ID in the parent and include
it in the title; after creation put the parent URL/marker in the README. On a
lost response search the exact title and inspect owner/README before retrying.
After every business write read back the object/field/item and checkpoint its
actual ID. Checkpoint publication only needs readback, not another checkpoint;
the state contract owns its retry and bootstrap exceptions.
Use the parent index and native links together. GitHub supports REST sub-issue
and issue-dependency endpoints; verify installed/live support and numeric issue
IDs before posting. If unsupported or unauthorized, retain explicit reciprocal
links and prerequisite conditions, record the limitation, and continue. Never
substitute a PR number for the numeric issue database ID.

PR reconciliation uses `gh pr view NUMBER --json
state,headRefOid,baseRefName,mergeCommit,mergedAt,statusCheckRollup,reviewDecision`
and `gh pr checks NUMBER`; investigate with `gh run view RUN --log-failed`.
Read required checks for the exact current head; no checks registered is
pending/unknown, never green. Preserve run URLs and SHAs with evidence. A
changed base may require fresh integration checks even when the head is stable.

Project scope failure is a recoverable infrastructure blocker: checkpoint
desired create/add/field/update operations, continue issue/PR work, then report
the single credential-specific action needed. This is not a reason to request
broad replacement credentials or abandon independent implementation. Resume
re-discovers existing objects and applies only still-pending operations.
