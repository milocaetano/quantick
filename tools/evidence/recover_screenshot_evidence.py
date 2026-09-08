"""Prepare bounded issue-comment bodies or recover their original bytes.

Preparation writes local files only. The coordinator publishes sequentially,
records actual comment IDs in the recovery manifest, and pins its full hash.
Recovery uses public raw GitHub comment bodies, never a screenshot summary.
"""

import argparse
import base64
import gzip
import hashlib
import json
from pathlib import Path
import re
import urllib.request

from verify_screenshot_evidence import MAX_ARTIFACT_BYTES, read_json, require, verify


# Q7's public recovery destination is pinned to its source-evidence issue.
REPOSITORY = "milocaetano/quantick"
RECOVERY_ISSUE_ID = 337
# Leave room for metadata and fencing around each ASCII base64 comment.
BLOCK_BYTES = 36_000
BASE64_BLOCK_MAX_CHARS = 4 * ((BLOCK_BYTES + 2) // 3)
# Bound each public comment request without automatic retry bursts.
HTTP_TIMEOUT_SECONDS = 30
# Allow the base64 body plus GitHub's JSON metadata without unbounded reads.
COMMENT_RESPONSE_MAX_BYTES = 128 * 1024


def sha(data):
    return hashlib.sha256(data).hexdigest()


def prepare(args):
    require(re.fullmatch(r"[0-9a-f]{40}", args.source_sha), "full source SHA required")
    bundle = (args.capture / "bundle.json").read_bytes()
    original = read_json(bundle)
    verify(read_json((args.capture / "manifest.json").read_bytes()), bundle,
           (args.capture / "screenshot.png").read_bytes())
    provenance = read_json(args.provenance.read_bytes())
    require(provenance["source_sha"] == args.source_sha, "publication provenance source association")
    require(original["environment"]["system"]["git_commit"] == args.source_sha,
            "publication capture source association")
    args.output.mkdir(parents=True, exist_ok=False)
    source_files = {"bundle.json": args.capture / "bundle.json",
                    "manifest.json": args.capture / "manifest.json", "provenance.json": args.provenance}
    if args.inspection:
        source_files["original-image-inspection.md"] = args.inspection
    manifest = {"version": 1, "repository": REPOSITORY, "issue": RECOVERY_ISSUE_ID,
                "source_sha": args.source_sha, "encoding": "gzip+base64", "artifacts": []}
    comments = 0
    for name, path in source_files.items():
        raw = path.read_bytes()
        compressed = gzip.compress(raw, mtime=0)
        artifact = {"name": name, "bytes": len(raw), "sha256": sha(raw),
                    "compressed_bytes": len(compressed), "compressed_sha256": sha(compressed), "chunks": []}
        blocks = [compressed[start:start + BLOCK_BYTES] for start in range(0, len(compressed), BLOCK_BYTES)]
        for index, block in enumerate(blocks):
            encoded = base64.b64encode(block).decode("ascii")
            require(len(encoded) <= BASE64_BLOCK_MAX_CHARS, "comment base64 budget")
            body_name = f"{name}.chunk-{index:04d}.md"
            body = (f"Q7 recoverable original bytes; source `{args.source_sha}`.\n\n"
                    f"Artifact: `{name}`; gzip chunk {index + 1}/{len(blocks)}.\n\n"
                    f"```q7-base64\n{encoded}\n```\n")
            (args.output / body_name).write_text(body, encoding="ascii", newline="\n")
            artifact["chunks"].append({"index": index, "bytes": len(block), "sha256": sha(block),
                                       "body_file": body_name, "comment_id": None})
            comments += 1
        manifest["artifacts"].append(artifact)
    png = base64.b64decode(original["screenshot"]["image_base64"], validate=True)
    manifest["embedded_png"] = {"name": "screenshot.png", "bytes": len(png), "sha256": sha(png)}
    (args.output / "manifest-draft.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": "PREPARED_LOCALLY", "projected_comments": comments,
                      "source_sha": args.source_sha,
                      "next": "Coordinator publishes bodies sequentially, records actual IDs, and pins final manifest hash."}, indent=2))


def recover(args):
    manifest_bytes = args.manifest.read_bytes()
    require(sha(manifest_bytes) == args.manifest_sha256, "pinned recovery manifest digest")
    manifest = read_json(manifest_bytes)
    require(manifest["repository"] == REPOSITORY and manifest["issue"] == RECOVERY_ISSUE_ID,
            "recovery repository/issue")
    require(manifest["version"] == 1 and manifest["encoding"] == "gzip+base64", "recovery format")
    args.output.mkdir(parents=True, exist_ok=False)
    reports, names = [], set()
    for artifact in manifest["artifacts"]:
        name = artifact["name"]
        require(name in {"bundle.json", "manifest.json", "provenance.json", "original-image-inspection.md"}
                and name not in names, "unknown or duplicate recovery artifact")
        names.add(name)
        compressed = bytearray()
        for index, chunk in enumerate(artifact["chunks"]):
            require(chunk["index"] == index and isinstance(chunk["comment_id"], int)
                    and chunk["comment_id"] > 0, "published comment ID/order required")
            url = f"https://api.github.com/repos/{REPOSITORY}/issues/comments/{chunk['comment_id']}"
            request = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json",
                                                          "User-Agent": "quantick-evidence-recovery"})
            # One request per chunk; HTTP failures propagate without retry bursts.
            with urllib.request.urlopen(request, timeout=HTTP_TIMEOUT_SECONDS) as response:
                raw_response = response.read(COMMENT_RESPONSE_MAX_BYTES + 1)
            require(len(raw_response) <= COMMENT_RESPONSE_MAX_BYTES, "comment response budget")
            (args.output / f"comment-{chunk['comment_id']}.json").write_bytes(raw_response)
            comment = read_json(raw_response)
            require(comment["issue_url"] == f"https://api.github.com/repos/{REPOSITORY}/issues/{RECOVERY_ISSUE_ID}",
                    "comment issue association")
            blocks = re.findall(r"(?m)^```q7-base64\r?\n([A-Za-z0-9+/=\r\n]+)^```[ \t]*$", comment["body"])
            require(len(blocks) == 1, "exactly one named base64 fence required")
            block = base64.b64decode(blocks[0].replace("\r", "").replace("\n", ""), validate=True)
            require(len(block) == chunk["bytes"] <= BLOCK_BYTES and sha(block) == chunk["sha256"],
                    "recovery chunk bytes/digest")
            compressed.extend(block)
        require(len(compressed) == artifact["compressed_bytes"]
                and sha(compressed) == artifact["compressed_sha256"], "compressed artifact bytes/digest")
        require(artifact["bytes"] <= MAX_ARTIFACT_BYTES, "recovered artifact byte budget")
        # The pinned manifest bounds output; do not inflate unbounded input.
        import io
        with gzip.GzipFile(fileobj=io.BytesIO(compressed)) as stream:
            raw = stream.read(artifact["bytes"] + 1)
        require(len(raw) == artifact["bytes"] and sha(raw) == artifact["sha256"], "original artifact bytes/digest")
        (args.output / name).write_bytes(raw)
        reports.append({"name": name, "bytes": len(raw), "sha256": sha(raw)})
    require({"bundle.json", "manifest.json", "provenance.json"} <= names, "required original artifacts")
    bundle = (args.output / "bundle.json").read_bytes()
    document = read_json(bundle)
    png = base64.b64decode(document["screenshot"]["image_base64"], validate=True)
    require(len(png) == manifest["embedded_png"]["bytes"] and sha(png) == manifest["embedded_png"]["sha256"],
            "recovered embedded original PNG")
    (args.output / "screenshot.png").write_bytes(png)
    verified = verify(read_json((args.output / "manifest.json").read_bytes()), bundle, png)
    provenance = read_json((args.output / "provenance.json").read_bytes())
    require(provenance["source_sha"] == manifest["source_sha"], "recovery provenance source association")
    require(document["environment"]["system"]["git_commit"] == manifest["source_sha"],
            "recovery capture source association")
    report = {"status": "PASS", "source_sha": manifest["source_sha"], "artifacts": reports,
              "embedded_png": manifest["embedded_png"], "verification": verified}
    (args.output / "readback-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    make = subparsers.add_parser("prepare")
    make.add_argument("--capture", type=Path, required=True)
    make.add_argument("--provenance", type=Path, required=True)
    make.add_argument("--inspection", type=Path)
    make.add_argument("--source-sha", required=True)
    make.add_argument("--output", type=Path, required=True)
    read = subparsers.add_parser("recover")
    read.add_argument("--manifest", type=Path, required=True)
    read.add_argument("--manifest-sha256", required=True)
    read.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    (prepare if args.command == "prepare" else recover)(args)


if __name__ == "__main__":
    main()
