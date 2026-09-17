"""Credential-bearing artifact transfer only. Never starts product processes."""
import json
import os
from pathlib import Path
import sys
import hashlib
import shutil
import time
import urllib.request
import zipfile

import owner_perf_runner01 as owner


def fetch(url, token):
    request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token, "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"})
    # urllib strips neither credentials on every cross-host redirect by
    # contract nor pins artifact destinations; use no automatic redirects.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, request, fp, code, message, headers, newurl):
            return None
    return urllib.request.build_opener(NoRedirect).open(request)


def file_identity(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return {"sha256": digest.hexdigest(), "bytes": path.stat().st_size}


def main(binding_path, directory=Path("C:/owner-transport")):
    receipt = {"state": "started", "phase": "binding", "utc_ns": time.time_ns()}
    try:
        transfer(binding_path, directory, receipt)
        receipt["state"] = "terminal"
    except BaseException as error:
        receipt.update(state="failed", error_type=type(error).__name__)
        if hasattr(error, "code") and isinstance(error.code, int):
            receipt["http_status"] = error.code
        if isinstance(error, owner.InvalidEvidence):
            receipt["reason"] = str(error)
        raise
    finally:
        receipt["files"] = {p.name: file_identity(p) for p in directory.iterdir() if p.is_file() and p.name != "transport.json"}
        owner.save(directory / "transport.json", receipt)


def transfer(binding_path, directory, receipt):
    import urllib.error
    binding = owner.load(binding_path)
    owner.require("PENDING" not in json.dumps(binding), "transport not released")
    expected = binding["artifact"]
    receipt["expected"] = {key: expected[key] for key in ("run_id", "artifact_id", "carrier_sha", "name", "download_sha256")}
    receipt["phase"] = "run_metadata"
    owner.save(directory / "transport.json", receipt)
    base = "https://api.github.com/repos/milocaetano/quantick/actions/"
    token = os.environ["OWNER_TRANSPORT_TOKEN"]
    with fetch(base + "runs/" + str(expected["run_id"]), token) as response:
        run = json.load(response)
    receipt["observed_run"] = {key: run.get(key) for key in ("id", "run_attempt", "head_sha", "conclusion")}
    owner.save(directory / "transport.json", receipt)
    owner.require(run["id"] == expected["run_id"] and run["run_attempt"] == 1 and run["head_sha"] == expected["carrier_sha"] and run["conclusion"] == "success", "wrong preparation run")
    receipt["phase"] = "artifact_metadata"
    owner.save(directory / "transport.json", receipt)
    with fetch(base + "artifacts/" + str(expected["artifact_id"]), token) as response:
        artifact = json.load(response)
    receipt["phase"] = "artifact_metadata"
    receipt["observed_artifact"] = {key: artifact.get(key) for key in ("id", "name", "expired", "digest")}
    receipt["observed_artifact"]["run_id"] = artifact.get("workflow_run", {}).get("id")
    owner.save(directory / "transport.json", receipt)
    owner.require(artifact["id"] == expected["artifact_id"] and not artifact["expired"] and artifact["name"] == expected["name"] and artifact["workflow_run"]["id"] == expected["run_id"] and artifact["digest"] == "sha256:" + expected["download_sha256"], "wrong artifact identity")
    receipt["phase"] = "redirect"
    owner.save(directory / "transport.json", receipt)
    try:
        fetch(base + "artifacts/" + str(expected["artifact_id"]) + "/zip", token)
        raise ValueError("artifact endpoint did not redirect")
    except urllib.error.HTTPError as redirect:
        owner.require(redirect.code == 302, "artifact transport refused")
        location = redirect.headers["Location"]
    owner.require(location.startswith("https://"), "insecure artifact redirect")
    # Signed blob URL is kept in memory, never logged; no GitHub token is sent.
    receipt["phase"] = "outer_download"
    owner.save(directory / "transport.json", receipt)
    archive = directory / "download.zip"
    with urllib.request.urlopen(location) as response, archive.open("xb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)
    receipt["outer"] = file_identity(archive)
    owner.save(directory / "transport.json", receipt)
    owner.require(receipt["outer"]["sha256"] == expected["download_sha256"], "download bytes changed")
    receipt["phase"] = "payload_member"
    owner.save(directory / "transport.json", receipt)
    with zipfile.ZipFile(archive) as z:
        matches = [i for i in z.infolist() if i.filename.endswith("/payload.zip") or i.filename == "payload.zip"]
        owner.require(len(matches) == 1, "payload missing or ambiguous")
        payload = directory / "payload.zip"
        with z.open(matches[0]) as source, payload.open("xb") as output:
            shutil.copyfileobj(source, output, length=1024 * 1024)
    receipt["phase"] = "payload_hash"
    receipt["payload"] = file_identity(payload)
    owner.save(directory / "transport.json", receipt)
    owner.require(receipt["payload"]["sha256"] == binding["package_sha256"], "payload digest mismatch")


if __name__ == "__main__":
    try:
        main(Path(sys.argv[1]))
    except BaseException:
        sys.exit(1)
