"""Replay stress run: a full recorded day at speed, a scripted hand, a verdict.

Plays one recorded session through a `--features harness` quantick-app at
`--speed` while `QUANTICK_STRESS_INPUT` pans, zooms and double-clicks the flow
pane every `--period-ms`, then grades the `STRESS_INPUT_SUMMARY` the app logs:

  * the session plays to its last print,
  * no frame interval at or over 250 ms, frame p99 under 50 ms,
  * every gesture answered within two frames, no input stall.

Everything the run reads is copied into `--out` first, and every store the app
could write points there, so the trader's own files are only ever read. The
app's stderr is read by this script; `--slow-sink` makes that reader stall
for `--stall-ms` every `--stall-every-ms`, the way a busy disk or a console
being drawn stalls a log file, to show whether a slow sink reaches the frame.

    python tools/stress/replay_stress.py --exe target/release/quantick-app.exe \
        --out %TEMP%/stress-main --session 2026-10-15

The process this script starts is the only one it stops.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time

HOME = Path(os.environ.get("USERPROFILE", Path.home())) / "Documents" / "Quantick"
STORES = {
    "QUANTICK_LAYOUTS": "layouts.toml",
    "QUANTICK_INDICATORS_STATE": "indicators-state.toml",
    "QUANTICK_INDICATOR_PRESETS": "indicator-presets.toml",
    "QUANTICK_CHART_LAYERS": "chart-layers.toml",
    "QUANTICK_BUBBLE_ASSETS": "bubble-assets.toml",
    "QUANTICK_DRAWING_PRESETS": "quantick-drawing-presets.toml",
    "QUANTICK_FOOTPRINT_SETTINGS": "footprint-settings.toml",
    "QUANTICK_FOOTPRINT_PRESETS": "footprint-presets.toml",
    "QUANTICK_CANDLE_PRESETS": "candle-presets.toml",
    "QUANTICK_SYMBOLS": "quantick-symbols.toml",
    "QUANTICK_PAPER_STATE": "paper-state.toml",
    "QUANTICK_STRATEGY_PRESETS": "quantick-strategies.toml",
}
# The trader's layout on the day the freeze was reported: candles by the day
# and by the minute beside a tick(50) flow pane, one tab, nothing else live.
UI_STATE = """version = 1
save_on_exit = false
window = [2560.0, 1369.3333740234375]
active_tab = 0
saved = []
favorite_tools = []
recent_workspaces = []

[[tabs]]
feed = "metatrader-b3"
symbol = "{symbol}"
layout = "time+time+flow"
split_fraction = 0.27
context_collapsed = false
flow_collapsed = false
context_heights = [0.53, 0.47]
context_panes_collapsed = [false, false]
focus = "flow"
focus_slot = 0
flow_bars = "{flow_bars}"
time_bars = "time:1d"
context_bars = ["time:1d", "time:1m"]
flow_layout = 1
context_layouts = [1, 1]
flow_legend_collapsed = false
time_legend_collapsed = false

[chrome]
timezone_minutes = -180
dock_visible = true
dock_tab = "trading"
rail_visible = true
rail_dock = "left"
perf_readings = true
progressive_history = true
history_reach = "sessions:5"
venue_lead_in = false
"""
FIELD = re.compile(r'(\w+)=("[^"]*"|\S+)')
ANSI = re.compile(r"\x1b\[[0-9;]*m")
FREEZE_MS = 250.0
P99_MS = 50.0
ANSWER_FRAMES = 2


def fields(line):
    line = ANSI.sub("", line)
    return {key: value.strip('"') for key, value in FIELD.findall(line)}


def prepare(args, repo):
    out = Path(args.out).resolve()
    store, tapes = out / "store", out / "replay" / args.symbol
    for folder in (store, tapes, out / "config", out / "trades", out / "deals"):
        folder.mkdir(parents=True, exist_ok=True)
    source = Path(args.tapes) if args.tapes else HOME / "replay" / args.symbol
    for tape in sorted(source.glob("*.csv")):
        target = tapes / tape.name
        if not target.exists() or target.stat().st_size != tape.stat().st_size:
            shutil.copy2(tape, target)
    for trace in tapes.glob("*.control-trace.jsonl"):
        trace.unlink()
    for name in STORES.values():
        if (HOME / name).exists():
            shutil.copy2(HOME / name, store / name)
    (store / "ui-state.toml").write_text(
        UI_STATE.format(symbol=args.symbol, flow_bars=args.flow_bars), newline="\n"
    )
    config = repo / "crates" / "app" / "config"
    for name in ("feeds.toml", "bubbles.toml", "instruments.toml", "chart-layers.toml"):
        shutil.copy2(config / name, out / "config" / name)
    feeds = out / "config" / "feeds.toml"
    # Never the trader's bridge port: a second listener on it steals his feed.
    feeds.write_text(
        feeds.read_text().replace('listen_addr = "127.0.0.1:9100"', 'listen_addr = "127.0.0.1:9187"'),
        newline="\n",
    )
    return out


def environment(args, out):
    env = {k: v for k, v in os.environ.items() if not k.startswith("QUANTICK_")}
    env.update({key: str(out / "store" / name) for key, name in STORES.items()})
    env.update(
        QUANTICK_UI_STATE=str(out / "store" / "ui-state.toml"),
        QUANTICK_CONFIG=str(out / "config" / "feeds.toml"),
        QUANTICK_INSTRUMENTS=str(out / "config" / "instruments.toml"),
        QUANTICK_TRADES_DIR=str(out / "trades"),
        QUANTICK_DEALS_DIR=str(out / "deals"),
        QUANTICK_REPLAY_DIR=str(out / "replay" / args.symbol),
        QUANTICK_REPLAY_SESSION=args.session,
        QUANTICK_REPLAY_AUTOSTART="1",
        QUANTICK_REPLAY_SPEED=str(args.speed),
        QUANTICK_STRESS_INPUT=str(args.period_ms),
        RUST_LOG="quantick=info",
        NO_COLOR="1",
    )
    return env


def run(args, out, env):
    log_path = out / "stderr.log"
    process = subprocess.Popen(
        [str(Path(args.exe).resolve())],
        cwd=out,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    print(f"launched pid {process.pid}; log {log_path}", flush=True)
    state = {"summary": None, "health": None, "ended": False, "stalls": []}

    def read():
        next_stall = time.monotonic() + args.stall_every_ms / 1000
        with open(log_path, "wb") as log:
            for raw in process.stderr:
                log.write(raw)
                line = raw.decode("utf-8", "replace")
                if "STRESS_INPUT_SUMMARY" in line:
                    state["summary"] = fields(line)
                elif "STRESS_INPUT_STALL" in line:
                    state["stalls"].append(fields(line).get("gesture"))
                elif "APP_HEALTH_SUMMARY" in line:
                    health = fields(line)
                    state["health"] = health
                    played, total = health.get("replay_played"), health.get("replay_total")
                    if played and played.isdigit() and played == total:
                        state["ended"] = True
                if args.slow_sink and time.monotonic() >= next_stall:
                    log.flush()
                    time.sleep(args.stall_ms / 1000)
                    next_stall = time.monotonic() + args.stall_every_ms / 1000

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    deadline = time.monotonic() + args.timeout_s
    ended_at = None
    while process.poll() is None and time.monotonic() < deadline:
        time.sleep(1)
        if state["ended"] and ended_at is None:
            ended_at = time.monotonic()
        # One more summary after the last print, so the totals cover it.
        if ended_at is not None and time.monotonic() - ended_at > 6:
            break
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=30)
    reader.join(timeout=10)
    return state


def verdict(state):
    s = state["summary"]
    if not s:
        return {"pass": False, "error": "no STRESS_INPUT_SUMMARY: is the exe a --features harness build?"}
    num = lambda key: float(s.get(key, "nan"))
    sent, on_time = int(num("sent")), int(num("on_time"))
    stalls, answered = int(num("stalls")), int(num("answered"))
    checks = {
        "played_to_end": state["ended"],
        "no_frame_over_250ms": num("freezes") == 0,
        "p99_under_50ms": num("p99_ms") < P99_MS,
        # The gesture in flight when the run stopped is neither answered nor stalled.
        "answered_within_2_frames": sent > 0 and on_time == answered and answered + stalls >= sent - 1,
        "zero_input_stalls": stalls == 0,
    }
    report = {
        "checks": checks,
        "pass": all(checks.values()),
        "frames": int(num("frames")),
        "freezes": int(num("freezes")),
        "worst_ms": num("worst_ms"),
        "p99_ms": num("p99_ms"),
        "gestures_sent": sent,
        "answered": answered,
        "answered_on_time": on_time,
        "slowest_answer_frames": int(num("slowest_answer_frames")),
        "stalls": stalls,
        "stalled_gestures": state["stalls"],
        "replay_played": (state["health"] or {}).get("replay_played"),
        "replay_total": (state["health"] or {}).get("replay_total"),
    }
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--exe", required=True, help="a --features harness quantick-app")
    parser.add_argument("--out", required=True, help="scratch folder; the trader's files are only copied")
    parser.add_argument("--symbol", default="WINZ26")
    parser.add_argument("--session", default="2026-10-15")
    parser.add_argument("--tapes", help="folder holding the session tapes (default: Documents/Quantick/replay/<symbol>)")
    parser.add_argument("--flow-bars", default="tick:50")
    parser.add_argument("--speed", type=float, default=50.0)
    parser.add_argument("--period-ms", type=int, default=3000)
    parser.add_argument("--slow-sink", action="store_true")
    parser.add_argument("--stall-ms", type=int, default=1000)
    parser.add_argument("--stall-every-ms", type=int, default=2000)
    parser.add_argument("--timeout-s", type=int, default=3600)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    out = prepare(args, repo)
    state = run(args, out, environment(args, out))
    report = verdict(state)
    (out / "verdict.json").write_text(json.dumps(report, indent=2), newline="\n")
    print(json.dumps(report, indent=2))
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
