## Summary

PR 4 of `docs/mcp-control-plane-development-plan.md`: `quantick-mcp`, the first adapter of the control plane — a local STDIO MCP server that an MCP client (Codex, Claude Code, …) launches as a subprocess. It discovers a Quantick instance that is **already running** with local agent access enabled, authenticates through `quantick-control-local`, and exposes a small fixed tool set over the gateway's observer capabilities. It never starts Quantick.

- `crates/mcp` (**new**, `quantick-mcp`, lib + binary) — `jsonrpc` (the line-delimited JSON-RPC 2.0 slice an MCP server needs), `protocol` (tools, annotations, results, version negotiation over `2025-06-18` / `2025-03-26` / `2024-11-05`), `link` (the `ControlLink` port and `LocalLink`, its implementation over discovery and the loopback client, with the instance-routing rules of contract §8), `tools` (the fixed set: `quantick_describe`, `quantick_get_snapshot`, `quantick_get_chart_window`, `quantick_get_diagnostics`, `quantick_search_capabilities`, `quantick_invoke`), `server` (the request loop), `setup` (the Codex / Claude Code registration assistant of contract §13), `fake` (a second `ControlLink` for tests).
- Tool input schemas are the committed contract documents under `schemas/control/`, embedded, with one routing property added; output schemas wrap the committed result documents in the envelope fields the adapter returns. A test validates every tool schema as a draft 2020-12 document.
- `workspace_deps` rule `mcp → control, control-local`; `CLAUDE.md` crate entry; `docs/control-plane/pr4-mcp-evidence.md`.

No MCP SDK and no async runtime: the protocol surface this server needs is a few hundred lines it owns, which is what keeps standard output provably clean (contract §8: stdout carries MCP frames only).

**Stacked on PR 3 (`feat/control-gateway`), itself stacked on #213.** Merge in order; GitHub retargets each PR when its base lands.

## Rate class and tier

Per tool call, in a separate process; nothing runs inside the application. Observe tier: the adapter requests the observer profile and read scopes, the gateway intersects them with the user's grant, and asking never grants. `--profile` accepts only `observer` in this release; anything else is refused before serving (exit 2, nothing on stdout).

## Instance selection and authority

Every instance-bound tool takes an optional `instance_id` that the adapter removes before the capability payload is validated. With exactly one live instance it is selected; with none the call is `control.instance_gone` with the discovery report's next steps; with several and no choice it is `control.instance_ambiguous` listing the choices in `(published_at_unix_ms, instance_id)` order — never the newest window. `--instance <id>` pins every call and refuses a contradicting routing ID. `quantick_describe` without an ID lists the live instances.

Under the observer ceiling `quantick_invoke` is annotated read-only and a write capability ID is refused by the instance, reported as a tool execution error with its `control.*` code. Named reads carry `readOnlyHint: true`, `destructiveHint: false`, `openWorldHint: false`; `quantick_invoke` takes the conservative hints of the profile ceiling (contract §8 table), pinned by test.

## Docking

- Port: `link::ControlLink` (`instances`, `invoke`). Two implementations: `LocalLink` over the real transport and `fake::FakeLink`; the server and tools see only the trait.
- Blast radius: added `crates/mcp/*` (12 files) and the evidence doc; edited only registration lines — workspace `Cargo.toml` members, `workspace_deps` `ALLOWED`, `CLAUDE.md`, the control-plane README index. No application file changes.
- Defaults preserve today's behaviour: the binary does nothing until a client launches it, and it connects to nothing until an instance has enabled access.

## Verification

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` — all exit 0 on the branch head.
- `cargo test -p quantick-mcp`: 24 unit tests; 4 integration tests against a fake loopback gateway that publishes a real descriptor and accepts the contract's handshake with the reference registry (discovery, handshake, describe, snapshot, search, refused write, wrong instance, two instances ordered and never chosen silently, a pinned adapter that refuses a contradicting ID and one whose listing names only its instance and fails when it is gone); 3 stdio smoke tests over the built binary (every stdout line is a JSON-RPC frame from startup through a garbage line to shutdown, exit 0, the instances directory never created; an unavailable profile refused with nothing on stdout; `setup` prints the contract's command with the binary path and no secret).
- Live check with Codex / Claude Code against a running desktop instance: **not run** — this session was not authorized to launch the desktop app. The registration commands are exactly the contract's; `quantick-mcp setup --client codex|claude` prints them.

## Architecture review

Step 0: `code-review` at `high` over `feat/mcp-observer` (its dispatched finders never reported back, so the reviewer ran the bug pass itself — stated here as the skill asks). Six findings, one confirmed and five plausible; all six resolved on the branch head (`fix(mcp): ...`), none deferred:

1. **Confirmed** — a tool error's `structuredContent` (`{ "error": { code, message, … } }`) did not match the tool's `outputSchema`; a client that validates structured results even on `isError` (the TypeScript SDK does) would reject every refused call. Every output schema is now a `oneOf` of the result document and an `ErrorResponse` branch built from the committed `control-error-v1` document, and `a_tool_error_validates_against_every_output_schema` pins it.
2. `quantick_get_chart_window` described later pages as "send the cursor it returned" while the capability needs the same query plus the cursor — wording corrected.
3. `LocalLink` rediscovered and re-authenticated on every call — it now reuses the connection when discovery still shows exactly the cached instance (`sole_advertised_instance`), and reconnects when the world changed.
4. The serve loop read standard input lines unbounded and assumed UTF-8 — `read_bounded_line` caps a frame at `CONTROL_MAX_REQUEST_BYTES`, answers an overlong or non-UTF-8 line with `PARSE_ERROR` and keeps serving (`an_overlong_or_non_utf8_line_is_answered_not_fatal`).
5. The search query limit compared bytes against a `maxLength` that JSON Schema counts in characters — `SEARCH_QUERY_MAX_CHARS`, counted in chars on both sides.
6. A request with `"id": null` was read as a notification and never answered, leaving the client waiting — now `INVALID_REQUEST` (tested); and `parse_args` refuses a flag of the other mode (`--client` under `serve`, `--instance` under `setup`) instead of silently ignoring it.

Second pass (`code-review 222 high` over the branch head after the fixes above, reviewer ran the pass itself): two confirmed and six plausible findings; two confirmed and four plausible resolved on the branch, two plausible deferred with reason:

1. **Confirmed** — `quantick_search_capabilities` matched snapshot scopes on a `scope_id` field the real describe document does not have (the contract's field is `id`), so a search by scope ID never found a scope against a running instance; only the fakes used `scope_id`. The search reads `id`; the fakes carry the contract's scope shape and `the_fake_describe_document_keeps_the_contracts_scope_shape` validates them against the committed describe-result document's `SnapshotScopeDescriptor`, so they cannot drift again; the server search test now finds `system.info` by ID.
2. **Confirmed** — a frame with an `id` but no string `method` and no `result`/`error` was classified as a client response and dropped, leaving the client waiting forever. It is `Message::Malformed`, answered `INVALID_REQUEST` with the id echoed (`a_frame_with_an_id_but_no_method_result_or_error_is_answered_not_dropped`).
3. `LocalLink::instances()` ignored the `--instance` pin, so a pinned adapter's `quantick_describe {}` listed every live instance and succeeded after the pinned one disappeared — the listing now honours the pin and fails with `control.instance_gone` when it is gone (`a_pinned_adapter_lists_only_its_instance_and_fails_when_it_is_gone`).
4. `read_bounded_line` propagated `Interrupted` from the blocking stdin read and ended the session — retried, as std's own line readers do.
5. Search results shipped each matching scope whole, schema included — scopes are projected to `id / module_id / title / description / schema_version / required_permissions`, the schema stays with `describe`.
6. The client cache was never pruned and a listing replaced cached connections — instances that are gone leave the cache, a live one keeps its connection (`entry().or_insert`).
7. `std::env::args()` panics on a non-Unicode argument — `args_os()` with a usage error (exit 2) instead.
8. The stdin frame bound reused the gateway's envelope bound, so a payload the gateway accepts could be refused for its MCP wrapping — `MCP_FRAME_MAX_BYTES` = envelope bound + named wrapper slack.

Deferred: the listing still connects and handshakes every advertised instance to prove liveness (descriptor presence alone is not liveness; a lazy variant is the adapter's housekeeping together with stale-descriptor cleanup, ADR 0001 §5); after a crash leaves a stale descriptor, `sole_advertised_instance` sees two candidates and the connection-reuse shortcut is skipped until the stale file is cleaned — same housekeeping item.

Shape: docking — the tool layer depends on the `ControlLink` port (two implementations; the server and tools never see a socket), the adapter docks on `quantick-control-local` + `quantick-control` only (`workspace_deps` rule `mcp → control, control-local`, `CLAUDE.md` updated), and adding a named tool is one `Tool` entry plus one `match` arm in `tools::call`; performance — per tool call in a separate process, nothing on the application's threads, discovery connects only to advertised instances; hardcoded — protocol versions, tool names, capability IDs and the routing property are named constants, limits come from `quantick_control::limits`; tests — 24 unit + 4 fake-gateway + 3 stdio smoke, each behaviour the plan names has one (`the_tool_list_is_fixed_and_named_as_the_contract_says`, `the_routing_id_never_reaches_the_instance…`, `routing_failures_and_refused_writes_are_tool_errors_with_their_codes`, `startup_errors_and_shutdown_emit_only_mcp_frames_on_stdout`, `two_live_instances_are_listed_in_order_and_never_chosen_silently`); standardisation — the same `control.*` error vocabulary as the gateway, the same schema documents; readability — module docs state each rule with its contract section; second operator — this crate *is* the second operator's door; it adds no trader-facing surface of its own.
