"""Root-approved sole-parent release derivation; never changes process guards."""
import copy
from datetime import datetime
import json
import os
from pathlib import Path
import subprocess
import time

import owner_perf_runner01 as owner


def query_identity():
    current, parent = os.getpid(), os.getppid()
    script = "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -in @(" + str(current) + "," + str(parent) + ") } | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId; parent=$_.ParentProcessId; name=$_.Name; created=$_.CreationDate.ToUniversalTime().ToString('o'); image=$_.ExecutablePath} } | ConvertTo-Json -Compress"
    rows = owner.strict_json(subprocess.check_output(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script], encoding="utf-8"))
    owner.require(type(rows) is list, "launcher identity rows unavailable")
    for row in rows:
        try:
            row["image_sha256"] = owner.digest(row["image"])
        except (OSError, TypeError):
            row["image_sha256"] = None
    return current, parent, rows


def validate(current, parent, rows, policy):
    own = [r for r in rows if r["pid"] == current]
    parents = [r for r in rows if r["pid"] == parent]
    owner.require(current != parent and len(rows) == 2 and len(own) == 1 and len(parents) == 1, "launcher identities ambiguous")
    own, selected = own[0], parents[0]
    owner.require(own["parent"] == parent and selected["name"].lower() == "pwsh.exe", "not immediate pwsh parent")
    for row, role in ((own, "python"), (selected, "pwsh")):
        owner.require(Path(row["image"]).resolve() == Path(policy[role + "_exe"]).resolve() and row["image_sha256"] == policy[role + "_sha256"], "launcher image identity mismatch")
    owner.require(datetime.fromisoformat(selected["created"]) <= datetime.fromisoformat(own["created"]), "launcher creation ordering invalid")
    return {"pid": parent, "created": selected["created"]}


def derive(template_path, expected_template_sha, policy_path, expected_policy_sha, output, event, observe=query_identity):
    output.mkdir(exist_ok=False)
    receipt = {"state": "started", "utc_ns": time.time_ns(), "event": {k: event[k] for k in ("run", "attempt", "before", "after", "ref")}, "observations": []}
    try:
        owner.require(owner.digest(template_path) == expected_template_sha and owner.digest(policy_path) == expected_policy_sha, "launcher template/policy changed")
        receipt.update(template_sha256=expected_template_sha, policy_sha256=expected_policy_sha)
        template, policy = owner.load(template_path), owner.load(policy_path)
        owner.require(template["allowed_processes"] == [] and "PENDING" not in json.dumps(policy), "launcher policy unresolved or template prepopulated")
        owner.require(policy["decision"].startswith("https://github.com/milocaetano/quantick/issues/480#issuecomment-"), "launcher policy authority absent")
        approved = None
        for _ in range(2):
            current, parent, rows = observe()
            public = [{key: row.get(key) for key in ("pid", "parent", "name", "created", "image_sha256")} for row in rows]
            public.sort(key=lambda row: row["pid"])
            receipt["observations"].append({"utc_ns": time.time_ns(), "current": current, "parent": parent, "rows": public})
            owner.save(output / "derivation.json", receipt)
            selected = validate(current, parent, rows, policy)
            identity = (current, parent, public)
            owner.require(approved is None or identity == approved, "launcher identity changed between observations")
            approved = identity
        derived = copy.deepcopy(template)
        derived["allowed_processes"] = [selected]
        owner.require({k for k in set(template) | set(derived) if template.get(k) != derived.get(k)} == {"allowed_processes"}, "unexpected derived release change")
        owner.require(owner.digest(template_path) == expected_template_sha, "template changed during derivation")
        (output / "root-template.json").write_bytes(template_path.read_bytes())
        path = output / "derived-release.json"
        owner.save(path, derived)
        receipt.update(state="terminal", derived_sha256=owner.digest(path), changed_keys=["allowed_processes"])
        return path
    except BaseException as error:
        receipt.update(state="failed", error_type=type(error).__name__)
        if isinstance(error, owner.InvalidEvidence):
            receipt["reason"] = str(error)
        raise
    finally:
        receipt["artifact_hashes"] = {p.name: owner.digest(p) for p in output.iterdir() if p.is_file() and p.name != "derivation.json"}
        owner.save(output / "derivation.json", receipt)
