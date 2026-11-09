# Pine library and source editor

Open **Indicators**, choose **Scripts**, then **View source**. Bundled indicators
are read-only. Enter a distinct name such as `my-rsi.pine` and choose **Create
editable copy**. Choose **Add to chart** to attach it through the picker's existing
path. Copies also appear in the script library and can be added to any chart.
The name is a stable identity; menus and source windows show readable titles.
Existing names, path separators and directory traversal are refused.

Editable source saves after 350 ms without typing. A bounded background validator
checks the current revision before the existing chart workers reload it. Editing
never adds another chart slot. Invalid source shows line/column diagnostics while
the chart continues displaying its last valid output. A later valid edit recovers.
Settings bind through the existing indicator reload port. Explicit saved overrides
survive source edits; untouched inputs adopt edited source defaults live and after
restart. Source files and layout settings survive reopening the workspace. Save is also available explicitly; wait
for pending validation to finish before closing the editor.

For an agent, discover `indicator.library.edit`. Its `operation.action` accepts
`list`, `open` (also reads source), `copy`, `add_to_chart` (with stable `id`),
`set_numeric_input`, `replace_source`, `save`, `inspect` and
`close`. `list`/`inspect` return up to 64 library entries per page (`offset`, `limit`),
the open document, revision, dirty/compiling state and diagnostics. Source is
limited to 256 KiB. The call requires cockpit/layout and user-text permissions.
Add-to-chart preserves library identity, file watching and workspace settings;
repeating it for the same identity on the focused pane is a no-op.
To commit a numerical setting through the same path as settings **Apply**, open
the attached identity on the focused pane and send, for example:

```json
{"operation":{"action":"set_numeric_input","id":"my-rsi.pine","name":"length","value":{"type":"int","value":4}}}
```

Use an input name returned by `numeric_inputs` and the exact `int` or `float` type. Values outside its
declared bounds or options are refused. Float values use canonical decimal strings,
such as `{"type":"float","value":"1.5"}`. Close any settings dialog and wait for
source validation before setting inputs. Repeating a value is safe, including
before worker output arrives. `numeric_inputs` returns up to 64 named committed
numeric values for the open focused attachment; `inspect` reconciles them.
Use an idempotency key when creating a copy; the same key replays its first result.
The journal records only editor status and revision, excluding private source.

External file saves retain the existing file-poll workflow. Bundled identities
cannot be shadowed by a same-named user file; create a distinct editable copy.
