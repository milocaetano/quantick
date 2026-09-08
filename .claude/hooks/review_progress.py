#!/usr/bin/env python3
"""Append-only, single-writer review checkpoints on a PR, accessed through gh."""

import json
import re
import subprocess
import sys

MARKER = "<!-- quantick-review-progress:v1 -->\n"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value):
    return type(value) is int and value >= 0


def evidence(value):
    return isinstance(value, list) and all(
        isinstance(item, str) and item.strip() for item in value
    )


def validate(record, previous=None):
    require(isinstance(record, dict), "Checkpoint must be an object.")
    require(set(record) == {
        "schema", "mission", "revision", "head", "stage", "batches", "limits",
        "findings", "evidence", "authorization", "stalled_batches",
    }, "Unknown or missing checkpoint fields.")
    require(type(record["schema"]) is int and record["schema"] == 1,
            "Unsupported checkpoint schema.")
    for name in ("mission", "stage"):
        require(isinstance(record[name], str) and record[name].strip(),
                f"Missing {name}.")
    require(isinstance(record["head"], str)
            and re.fullmatch(r"[0-9a-f]{40}", record["head"]), "Invalid head SHA.")
    require(integer(record["revision"]) and record["revision"] > 0
            and integer(record["batches"]), "Invalid cumulative counters.")
    require(integer(record["stalled_batches"])
            and record["stalled_batches"] <= record["batches"], "Invalid stall counter.")
    require(evidence(record["evidence"]) and record["evidence"],
            "Explicit history or transition evidence is required; never infer zero.")
    grant = record["authorization"]
    require(grant is None or isinstance(grant, str) and grant.strip(),
            "Invalid authorization evidence.")
    limits = record["limits"]
    require(isinstance(limits, dict) and set(limits) == {"batches", "attempts"}
            and all(integer(n) and n > 0 for n in limits.values()),
            "Invalid finite limits.")
    require(all(n <= 3 for n in limits.values()) or grant,
            "Limits above three require explicit authorization evidence.")
    findings = record["findings"]
    require(isinstance(findings, dict), "Findings must retain their stable IDs.")
    for key, finding in findings.items():
        require(isinstance(key, str) and key.strip() and isinstance(finding, dict),
                "Invalid finding identity.")
        require(set(finding) == {"attempts", "disposition", "evidence"}
                and integer(finding["attempts"])
                and finding["disposition"] in ("open", "fixed", "accepted", "deferred")
                and evidence(finding["evidence"]), "Invalid finding record.")
        require(finding["disposition"] == "open" or finding["evidence"],
                "A finding disposition needs evidence.")
    if previous is None:
        require(record["revision"] == 1, "Checkpoint history must begin at revision 1.")
        return
    require(record["mission"] == previous["mission"], "Mission identity cannot change.")
    require(record["revision"] == previous["revision"] + 1,
            "Checkpoint revision must advance by one.")
    require(record["batches"] >= previous["batches"], "Batch count cannot reset.")
    for key, old in previous["findings"].items():
        require(key in findings and findings[key]["attempts"] >= old["attempts"],
                "Finding IDs and cumulative attempts cannot reset.")
    newly_closed = any(
        old["disposition"] == "open"
        and findings[key]["disposition"] in ("fixed", "accepted")
        for key, old in previous["findings"].items()
    )
    expected_stalls = previous["stalled_batches"] + record["batches"] - previous["batches"]
    require(record["stalled_batches"] == expected_stalls
            or record["stalled_batches"] == 0 and newly_closed,
            "Reserve each batch in the stall count; reset only with a proven closure.")
    if any(limits[key] > previous["limits"][key] for key in limits):
        require(grant and grant != previous["authorization"],
                "Increasing a limit requires new authorization evidence.")


def gh(*args, payload=None):
    result = subprocess.run(
        ["gh", *args], input=None if payload is None else json.dumps(payload),
        text=True, encoding="utf-8", capture_output=True, check=False,
    )
    require(result.returncode == 0,
            "GitHub operation uncertain or failed; read back before retrying.")
    return json.loads(result.stdout)


def history(endpoint):
    pages = gh("api", endpoint, "--paginate", "--slurp")
    require(isinstance(pages, list) and all(isinstance(p, list) for p in pages),
            "Incomplete comment pagination.")
    records = {}
    for page in pages:
        for comment in page:
            require(isinstance(comment, dict) and isinstance(comment.get("body"), str),
                    "Incomplete comment response.")
            body = comment["body"]
            if not body.startswith("<!-- quantick-review-progress:"):
                continue
            require(body.startswith(MARKER), "Unknown progress marker version.")
            record = json.loads(body[len(MARKER):])
            require(isinstance(record, dict) and integer(record.get("revision")),
                    "Malformed progress checkpoint.")
            revision = record["revision"]
            require(revision not in records or records[revision] == record,
                    "Conflicting checkpoint revisions; reconcile concurrent writers.")
            records[revision] = record
    previous = None
    for revision in sorted(records):
        record = records[revision]
        validate(record, previous)
        previous = record
    return previous


def record_checkpoint(endpoint, record):
    require(isinstance(record, dict), "Checkpoint must be an object.")
    previous = history(endpoint)
    if previous == record:
        return previous
    validate(record, previous)
    # Never retry a failed POST here: it may have reached GitHub. An identical
    # retry first reads back and returns the saved revision without writing.
    gh("api", endpoint, "--method", "POST", "--input", "-",
       payload={"body": MARKER + json.dumps(record, sort_keys=True)})
    confirmed = history(endpoint)
    require(confirmed == record, "Checkpoint write not confirmed; reconcile before work.")
    return confirmed


def check_repair(record, finding_ids):
    require(record is not None, "No durable history; establish observed counts first.")
    require(finding_ids, "Name the stable finding IDs for this repair batch.")
    require(record["batches"] < record["limits"]["batches"], "Repair batch limit reached.")
    require(record["stalled_batches"] < 2, "Two batches without closure require escalation.")
    for key in finding_ids:
        finding = record["findings"].get(key)
        require(finding is not None, "Record the finding before dispatching repair.")
        require(finding["disposition"] == "open", "Only open findings can be repaired.")
        require(finding["attempts"] < record["limits"]["attempts"],
                "Finding repair attempt limit reached.")


def main(args):
    require(len(args) >= 2 and args[0] in ("show", "record", "check")
            and args[1].isdigit() and int(args[1]) > 0,
            "Usage: review_progress.py show|record|check PR [FINDING_ID ...]")
    operation, pr = args[:2]
    require(operation == "check" or len(args) == 2, "Unexpected arguments.")
    repo = gh("repo", "view", "--json", "nameWithOwner")["nameWithOwner"]
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo), "Invalid repository.")
    identity = gh("pr", "view", pr, "--json", "number")
    require(identity.get("number") == int(pr), "PR identity could not be verified.")
    endpoint = f"repos/{repo}/issues/{pr}/comments"
    if operation == "record":
        record = record_checkpoint(endpoint, json.load(sys.stdin))
    else:
        record = history(endpoint)
        if operation == "check":
            check_repair(record, args[2:])
    print(json.dumps(record, sort_keys=True))


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f"Review progress unavailable: {error}", file=sys.stderr)
        sys.exit(2)
