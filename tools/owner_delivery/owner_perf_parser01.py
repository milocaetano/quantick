"""Strict owner-delivery parser. Synthetic tests are not performance evidence."""
import json
import math
import re
import statistics
import struct

SELECTORS = {
    "timing": "app::tests::drawings_tests::owner_action_release_frame_measurement",
    "allocations": "app::tests::drawings_tests::owner_action_release_frame_allocations",
    "dense": "app::tests::control_plane_tests::control_idle_dense_replay_benchmark",
    "long": "app::tests::session_length_tests::long",
}
ACTIONS = {"fixed-range-profile": 2, "fib-retracement": 2, "fib-extension": 3}
ORDER = ("B1", "C1", "C2", "B2", "B3", "C3")
ALLOC_KEYS = {"allocs", "alloc_bytes", "reallocs", "realloc_copy_bytes", "largest_realloc_copy"}
LONG_PATHS = ("trade.chart.backfilled", "trade.chart.live", "trade.book", "depth.book", "frame.book", "frame.app.tick50", "frame.worker.tick50", "frame.app.time1d", "frame.worker.time1d")
LONG_BANNER = "sessions: 18000 and 3960000 prints at 300/s; window 1800 frames x 5 prints, 30000 depth updates; tolerance counts +10%, time +50%"


class InvalidEvidence(ValueError):
    pass


def require(condition, code):
    if not condition:
        raise InvalidEvidence(code)


def strict_json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result

    def constant(_):
        raise InvalidEvidence("nonfinite JSON constant")

    return json.loads(text, object_pairs_hook=pairs, parse_constant=constant)


def keys(value, expected):
    require(type(value) is dict and set(value) == set(expected), "unexpected object shape")


def integer(value, maximum=None):
    require(type(value) is int and value >= 0, "expected unsigned integer")
    require(maximum is None or value <= maximum, "integer outside domain")


def harness(text, mode):
    require(text.count("running 1 test") == 1, "not exactly one test")
    summaries = re.findall(r"^test result: (.+)$", text, re.M)
    require(len(summaries) == 1, "missing or duplicate terminal summary")
    require(re.fullmatch(r"ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in \d+(?:\.\d+)?s", summaries[0]) is not None,
            "test did not pass exactly once")
    require(text.count("test " + SELECTORS[mode] + " ...") == 1, "wrong selector")


def payloads(text, marker, mode):
    output = []
    prefix = "test " + SELECTORS[mode] + " ... "
    for line in text.splitlines():
        if marker not in line:
            continue
        if line.startswith(prefix):
            line = line[len(prefix):]
        require(line.startswith(marker + " "), "misplaced record marker")
        output.append(line[len(marker) + 1:])
    return output


def action_records(text, mode):
    require(mode in ("timing", "allocations"), "wrong action mode")
    harness(text, mode)
    records = [strict_json(p) for p in payloads(text, "OWNER_ACTION", mode)]
    require(len(records) == 330, "action count must be 330")
    for row, (action, index) in zip(records, ((a, i) for a in ACTIONS for i in range(110)), strict=True):
        keys(row, {"action", "observation", "warmup", "mode", "metric", "output"})
        require(row["action"] == action and row["mode"] == mode, "action/mode/order mismatch")
        integer(row["observation"])
        require(row["observation"] == index, "missing/duplicate/reordered index")
        require(type(row["warmup"]) is bool and row["warmup"] == (index < 10), "wrong warmup")
        keys(row["metric"], {"release_frame_ns"} if mode == "timing" else ALLOC_KEYS)
        for value in row["metric"].values():
            integer(value)
        out = row["output"]
        keys(out, {"tool", "points", "feed_id", "symbol", "pane"})
        require(out["tool"] == action and out["pane"] == "flow", "wrong semantic recipient")
        require(all(type(out[k]) is str and out[k] for k in ("feed_id", "symbol")), "missing market identity")
        require(type(out["points"]) is list and len(out["points"]) == ACTIONS[action], "wrong anchors")
        for point in out["points"]:
            keys(point, {"bar_bits", "price_bits", "time_ms"})
            for key in ("bar_bits", "price_bits"):
                width, floating = ("!I", "!f") if key == "bar_bits" else ("!Q", "!d")
                integer(point[key], 2**(32 if key == "bar_bits" else 64) - 1)
                require(math.isfinite(struct.unpack(floating, struct.pack(width, point[key]))[0]), "nonfinite anchor")
            require(point["time_ms"] is None or (type(point["time_ms"]) is int and -(2**63) <= point["time_ms"] < 2**63), "invalid timestamp")
        for point, expected in zip(out["points"][:2], (80.5, 160.5), strict=True):
            bar = struct.unpack("!f", struct.pack("!I", point["bar_bits"]))[0]
            require(abs(bar - expected) < 0.01, "fractional anchor mismatch")
        if action == "fib-extension":
            require(out["points"][1] == out["points"][2], "projection anchor mismatch")
    return records


def dense_record(text):
    harness(text, "dense")
    rows = payloads(text, "CONTROL_IDLE_DENSE_REPLAY", "dense")
    require(len(rows) == 1, "dense summary count")
    # The existing Rust source prints Option with Debug, not JSON. Only this
    # exact field spelling is translated; arbitrary Rust syntax is rejected.
    raw = rows[0]
    pattern = r'"feed_arrival_ms":(None|Some\((\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)\))'
    require(len(re.findall(pattern, raw)) == 1, "dense arrival Option missing")
    raw = re.sub(pattern, lambda m: '"feed_arrival_ms":' + (m[2] or "null"), raw)
    row = strict_json(raw)
    keys(row, {"frame_cpu_ms", "frame_p99_ms", "frame_worst_ms", "feed_arrival_ms", "trades_per_s", "frames", "trades_per_frame"})
    require(type(row["frames"]) is int and row["frames"] == 600 and type(row["trades_per_frame"]) is int and row["trades_per_frame"] == 64, "dense workload mismatch")
    for key in ("frame_cpu_ms", "frame_p99_ms", "frame_worst_ms", "trades_per_s", "feed_arrival_ms"):
        value = row[key]
        if key == "feed_arrival_ms" and value is None:
            continue
        require(type(value) in (float, int) and math.isfinite(value) and value >= 0, "invalid dense metric")
    return row


def long_record(text):
    harness(text, "long")
    require(text.count("# session-length long variant") == 1, "wrong long variant")
    require(len(re.findall(r"^verdict: within budget \(\d+ s\)$", text, re.M)) == 1, "long budget failure")
    require("OVER BUDGET" not in text, "long over budget")
    # Literal count/time/fold/lane budget evaluation stays in the unchanged
    # source-bound executable. Its full tables and session banner are retained.
    require(re.findall(r"^sessions: .+$", text, re.M) == [LONG_BANNER], "long workload mismatch")
    rows = []
    for line in text.splitlines():
        cells = [c.strip() for c in line.split("|")]
        if len(cells) > 2 and cells[1] in LONG_PATHS:
            require(len(cells) == 12, "long row shape")
            require(all(re.fullmatch(r"\d+(?:\.\d+)?", c) for c in cells[2:-1]), "long row metric")
            rows.append((cells[1], cells[2]))
    require(rows == [(path, str(session)) for path in LONG_PATHS for session in (18000, 3960000)], "missing/duplicate long path/session")
    growth = re.findall(r"^\| (18000|3960000) \| \d+\.\d+ \| \d+ \|$", text, re.M)
    require(growth == ["18000", "3960000"], "missing growth rows")
    return {"verdict": "within budget", "raw_output_retained": True}


def parse(text, mode):
    if mode in ("timing", "allocations"):
        return action_records(text, mode)
    return dense_record(text) if mode == "dense" else long_record(text)


def stats(values):
    ordered = sorted(values)
    return {"mean": statistics.mean(values), "median": statistics.median(values),
            "p99": ordered[math.ceil(len(ordered) * .99) - 1], "worst": ordered[-1]}


def compare_actions(runs):
    require(list(runs) == list(ORDER), "finite invocation order mismatch")
    reference = runs["B1"]
    for rows in runs.values():
        require(len(rows) == 330, "mode must total 1980 records")
        for expected, actual in zip(reference, rows, strict=True):
            require(expected["action"] == actual["action"] and expected["observation"] == actual["observation"] and expected["output"] == actual["output"], "semantic identity mismatch")
    result = {}
    for action in ACTIONS:
        result[action] = {}
        for run, rows in runs.items():
            measured = [r for r in rows if r["action"] == action and not r["warmup"]]
            require(len(measured) == 100, "retained count mismatch")
            result[action][run] = {key: stats([r["metric"][key] for r in measured]) for key in measured[0]["metric"]}
        result[action]["paired_delta_candidate_minus_baseline"] = {
            str(pair): {metric: {stat: result[action][f"C{pair}"][metric][stat] - result[action][f"B{pair}"][metric][stat]
                                for stat in ("mean", "median", "p99", "worst")}
                       for metric in result[action][f"B{pair}"]}
            for pair in (1, 2, 3)
        }
    return result
