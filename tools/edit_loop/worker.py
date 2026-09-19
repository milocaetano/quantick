"""Private supervised worker: wait for ownership, run Cargo, retain the leader."""

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import time

import inputs

# Linux prctl operation: adopt orphaned descendants instead of losing their
# ownership when Cargo or an intermediate test process exits.
PR_SET_CHILD_SUBREAPER = 36


def main():
    output = Path(sys.argv[1])
    # The parent assigns the Windows job / verifies the Linux session before
    # sending this line. No descendant is started before that handshake.
    command = json.loads(sys.stdin.readline())
    record = {"command": command, "started_utc": datetime.now(timezone.utc).isoformat(),
              "state": "starting", "started_perf_ns": time.perf_counter_ns()}
    inputs.write_json(output / "process.json", record)
    try:
        if os.name != "nt":
            import ctypes
            libc = ctypes.CDLL(None, use_errno=True)
            libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong,
                                   ctypes.c_ulong, ctypes.c_ulong]
            libc.prctl.restype = ctypes.c_int
            if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
                raise OSError(ctypes.get_errno(), "cannot retain orphaned descendant ownership")
        with (output / "stdout.log").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
            start = time.perf_counter_ns()
            process = subprocess.Popen(command, stdout=stdout, stderr=stderr)
            record.update(pid=process.pid, state="running", started_perf_ns=start)
            inputs.write_json(output / "process.json", record)
            record["exit_code"] = process.wait()
            record["elapsed_seconds"] = (time.perf_counter_ns() - start) / 1_000_000_000
        record.update(state="finished", finished_utc=datetime.now(timezone.utc).isoformat())
    except BaseException as error:
        record.update(state="failed", error=f"{type(error).__name__}: {error}")
    inputs.write_json(output / "process.json", record)
    print(json.dumps(record), flush=True)
    # Keep the session leader alive until the owner proves/terminates the tree.
    sys.stdin.readline()


if __name__ == "__main__":
    main()
