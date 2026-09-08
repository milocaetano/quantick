"""Retrieve real egui evidence from an already launched, explicitly owned PID.

This tool starts only the MCP adapter. Build/launch isolation and source/binary
provenance are separate prerequisites in screenshot-evidence-proof.md.
"""

import argparse
import base64
import datetime
import json
from pathlib import Path
import queue
import re
import subprocess
import threading

from verify_screenshot_evidence import digest, read_json, require, verify


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write_json(path, value):
    with path.open("x", encoding="utf-8", newline="\n") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


class Mcp:
    def __init__(self, executable, output):
        self.output = output
        self.sequence = 0
        self.lines = queue.Queue()
        self.stderr = (output / "mcp.stderr").open("xb")
        self.process = subprocess.Popen(
            [str(executable), "serve", "--profile", "observer"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        while line := self.process.stdout.readline():
            self.lines.put(line)
        self.lines.put(None)

    def request(self, method, params, notification=False):
        self.sequence += 1
        request = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notification:
            request["id"] = self.sequence
        stem = self.output / f"{self.sequence:04d}"
        raw = json.dumps(request, separators=(",", ":")).encode("utf-8") + b"\n"
        stem.with_suffix(".request.jsonl").write_bytes(raw)
        started = utc()
        self.process.stdin.write(raw)
        self.process.stdin.flush()
        if notification:
            return None
        # One bounded wait. Failure is surfaced, never retried automatically.
        try:
            line = self.lines.get(timeout=45)
        except queue.Empty as error:
            raise TimeoutError(f"MCP response timeout: {method}") from error
        require(line is not None, "MCP closed before response")
        stem.with_suffix(".response.jsonl").write_bytes(line)
        write_json(stem.with_suffix(".receipt.json"), {
            "started_utc": started, "finished_utc": utc(), "method": method,
            "request_digest": digest(raw), "response_digest": digest(line)})
        response = read_json(line)
        require(response.get("id") == self.sequence, "MCP response ID")
        require("error" not in response, f"MCP error: {response.get('error')}")
        return response["result"]

    def call(self, name, arguments):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        require(not result.get("isError", False), f"MCP tool failed: {name}")
        return result["structuredContent"]

    def close(self):
        try:
            self.process.stdin.close()
        except (BrokenPipeError, OSError):
            # Preserve the original capture failure if the adapter already died.
            pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.stderr.close()


def capture(args):
    require(re.fullmatch(r"[0-9a-f]{40}", args.source_sha), "full lowercase source SHA required")
    args.output.mkdir(parents=True, exist_ok=False)
    protocol = args.output / "protocol"
    protocol.mkdir()
    write_json(args.output / "capture-start.json", {
        "started_utc": utc(), "owned_pid": args.pid, "source_sha": args.source_sha,
        "mcp_executable_digest": digest(args.mcp.read_bytes()),
        "predeclared_canvas_edge_tolerance_px": 3,
        "source_provenance": "Must be corroborated by separate exact-source build/launch receipts."})
    mcp = Mcp(args.mcp, protocol)
    try:
        mcp.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                   "clientInfo": {"name": "Q7 evidence capture", "version": "1"}})
        mcp.request("notifications/initialized", {}, notification=True)
        discovered = mcp.call("quantick_describe", {})
        selected = [item for item in discovered["instances"] if item["process_id"] == args.pid]
        require(len(selected) == 1, "exactly one described instance must match owned PID")
        instance = selected[0]["instance_id"]
        write_json(args.output / "owned-instance.json", selected[0])

        def call(name, payload):
            result = mcp.call(name, {"instance_id": instance, **payload})
            require(result["instance_id"] == instance, "MCP routed instance association")
            return result["result"]

        write_json(args.output / "describe.json", call("quantick_describe", {}))
        write_json(args.output / "scene-before.json", call("quantick_get_scene", {}))
        write_json(args.output / "diagnostics-before.json", call("quantick_get_diagnostics", {}))
        manifest = call("quantick_capture_evidence", {"scopes": ["system.info", "workspace.summary",
                        "feed.status", "chart.summary", "health.summary", "scene.controls"], "screenshot": True})
        write_json(args.output / "manifest.json", manifest)
        require(manifest["environment"]["system"]["git_commit"] == args.source_sha,
                "capture build-environment source SHA differs from requested source")
        require(manifest.get("screenshot") is not None, "capture has no real egui screenshot")
        bundle = bytearray()
        cursor, next_index, seen_cursors = None, 0, set()
        while True:
            payload = {"evidence_id": manifest["evidence_id"]}
            if cursor is not None:
                payload["cursor"] = cursor
            page = call("quantick_invoke", {"capability_id": "evidence.read", "payload": payload})
            for field in ("evidence_id", "resource_id", "content_digest", "encoded_bytes", "chunk_count"):
                require(page[field] == manifest[field], f"page/manifest association: {field}")
            require(page["page"]["items"], "empty retained evidence page")
            for chunk in page["page"]["items"]:
                raw = base64.b64decode(chunk["data"], validate=True)
                require(chunk["index"] == next_index, "chunk index/order")
                require(int(chunk["byte_offset"]) == len(bundle), "chunk byte offset")
                require(int(chunk["byte_length"]) == len(raw), "chunk byte length")
                require(digest(raw) == chunk["digest"] == manifest["chunk_digests"][next_index], "chunk digest")
                bundle.extend(raw)
                next_index += 1
                require(len(bundle) <= int(manifest["encoded_bytes"]), "bundle exceeds declared bytes")
            cursor = page["page"].get("next_cursor")
            if cursor is None:
                break
            key = json.dumps(cursor, sort_keys=True)
            require(key not in seen_cursors, "repeated evidence cursor")
            seen_cursors.add(key)
        require(next_index == manifest["chunk_count"], "incomplete chunk count")
        original = bytes(bundle)
        (args.output / "bundle.json").write_bytes(original)
        document = read_json(original)
        png = base64.b64decode(document["screenshot"]["image_base64"], validate=True)
        (args.output / "screenshot.png").write_bytes(png)
        report = verify(manifest, original, png)
        report.update(source_sha=args.source_sha, finished_utc=utc())
        write_json(args.output / "verifier-report.json", report)
        print(json.dumps(report, indent=2))
    finally:
        mcp.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mcp", type=Path, required=True)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    capture(args)


if __name__ == "__main__":
    main()
