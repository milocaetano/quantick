# Pine library and source editor

Open **Indicators**, choose **Scripts**, then **View source**. Bundled indicators
are read-only. Enter a distinct name such as `my-rsi.pine` and choose **Create
editable copy**. Copies appear in the script library and can be added to any chart.
The name is a stable identity; menus and source windows show readable titles.
Existing names, path separators and directory traversal are refused.

Editable source saves after 350 ms without typing. A bounded background validator
checks the current revision before the existing chart workers reload it. Editing
never adds another chart slot. Invalid source shows line/column diagnostics while
the chart continues displaying its last valid output. A later valid edit recovers.
Settings bind through the existing indicator reload port. Source files and layout
settings survive reopening the workspace. Save is also available explicitly; wait
for pending validation to finish before closing the editor.

For an agent, discover `indicator.library.edit`. Its `operation.action` accepts
`list`, `open` (also reads source), `copy`, `replace_source`, `save`, `inspect` and
`close`. `list`/`inspect` return up to 64 library entries per page (`offset`, `limit`),
the open document, revision, dirty/compiling state and diagnostics. Source is
limited to 256 KiB. The call requires cockpit/layout and user-text permissions.
Use an idempotency key when creating a copy; the same key replays its first result.
The journal records only editor status and revision, excluding private source.

External file saves retain the existing file-poll workflow. Bundled identities
cannot be shadowed by a same-named user file; create a distinct editable copy.
