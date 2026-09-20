# Reading the running app through the control plane

Detail for `ui-harness` *Ask the app, then look*. Read it when you drive
`quantick-mcp` by hand.

## The fixture

Launch with local access and the scopes the read needs. The scope IDs are the
`QUANTICK_CONTROL_ACCESS` / `QUANTICK_CONTROL_SCOPES` /
`QUANTICK_CONTROL_EVIDENCE` rows of `hook-registry.md`:

```powershell
$env:QUANTICK_CONTROL_ACCESS = "1"
$env:QUANTICK_CONTROL_SCOPES = "all-reads,analyst-tier"
```

## The client

`quantick-mcp` is a STDIO MCP server; JSON-RPC lines on stdin are a complete
client. It discovers the running instance and never starts one. Build it into
the same target directory the launch used (the launch builds `quantick-app`
only):

```powershell
$target = "D:\quantick-agent-target"
cargo build -p quantick-mcp
$mcp = Join-Path $target "debug\quantick-mcp.exe"
$lines = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ui-harness","version":"1"}}}',
  '{"jsonrpc":"2.0","method":"notifications/initialized"}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"quantick_get_scene","arguments":{}}}'
)
```

**Send a blank line first.** Windows PowerShell 5.1 writes a UTF-8 preamble to
a child's stdin the moment `Process.StandardInput` is touched; it lands on line
1 and `initialize` answers `-32700 parse error: expected value at line 1 column
1` while every later line parses. A leading newline absorbs it:

```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $mcp; $psi.Arguments = "--profile analyst"
$psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true
$psi.UseShellExecute = $false
$m = [System.Diagnostics.Process]::Start($psi)
$nl = [char]10
$bytes = [System.Text.Encoding]::ASCII.GetBytes($nl + ($lines -join $nl) + $nl)
$m.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
$m.StandardInput.BaseStream.Flush(); $m.StandardInput.Close()
$m.StandardOutput.ReadToEnd()
```

**One instance at a time**, or discovery answers `control.instance_ambiguous`
with the ids. Clear strays by **path**, never by process name — `Get-Process
quantick-app | Stop-Process` also kills the trader's window.

Each answer is one JSON line: `result.structuredContent` is the capability's
result; `result.isError` with a `control.*` code is a refusal to branch on.

| Ask | Call |
| --- | --- |
| What is on screen, by name | `quantick_get_scene` |
| Which market, bars, layout | `quantick_get_snapshot` with the scopes |
| Is the frame healthy, the tape late | `quantick_get_diagnostics` |
| What changed since I looked | `quantick_read_events` / `quantick_wait_for_change` |
| Everything at one instant, hashed | `quantick_capture_evidence` |

## Evidence bundles

`quantick_capture_evidence` freezes the named scopes, surrounding events and
effective configuration into one hashed bundle and answers with a manifest.
Read it back with `quantick_invoke` on `evidence.read`, page by page,
concatenating the base64 chunks: the bytes are the bundle's canonical JSON and
their SHA-256 is the manifest's `content_digest`. Two fields decide whether an
assertion is sound:

- `coverage` — what the capture left out, as codes: unnamed scopes in
  `omitted_scopes`, unfillable fields in `unavailable_fields` with a JSON
  Pointer. `complete` is never true.
- `screenshot.capture_revision` equals the bundle's `capture_revision`, which
  makes `screenshot.control_regions` trustworthy: each named control's
  rectangle in the image, in physical pixels, with `within_image` saying
  whether the window clipped it.

Without a client, `QUANTICK_CONTROL_EVIDENCE` takes the same capture at launch
and logs the manifest as `CONTROL_EVIDENCE_CAPTURED`. Bundles live in memory
for fifteen minutes, are cleared when access is turned off, and never touch
disk — quote the evidence ID and numbers in a report instead.
