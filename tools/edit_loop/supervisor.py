"""Supervise the private worker while timing only its Cargo subprocess."""

from datetime import datetime, timezone
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading

import inputs
from process_owner import OwnedTree, ProcessStillRunning, STOP_TIMEOUT_SECONDS


def run_process(command, repo, env, output, timeout_seconds):
    record = {"command": command, "started_utc": datetime.now(timezone.utc).isoformat(),
              "state": "starting", "exit_code": None, "elapsed_seconds": None,
              "timed_out": False, "interrupted": None, "tree_quiescent": False}
    try:
        return supervise(command, repo, env, output, timeout_seconds, record)
    except BaseException as error:
        # Even an I/O error while closing pipes or saving failure evidence must
        # not turn unknown process ownership into ordinary safe restoration.
        if not record["tree_quiescent"]:
            raise ProcessStillRunning("owned tree is not proven quiescent") from error
        raise


def supervise(command, repo, env, output, timeout_seconds, record):
    worker = Path(__file__).with_name("worker.py")
    owner = OwnedTree()
    process = None
    reader = None
    interrupted = None
    messages = queue.Queue()
    try:
        with (output / "supervisor.log").open("wb") as diagnostic:
            process = subprocess.Popen([sys.executable, str(worker), str(output)], cwd=repo, env=env,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=diagnostic,
                                       text=True, encoding="utf-8", **owner.options())
            owner.attach(process)
            record["supervisor_pid"] = process.pid
            inputs.write_json(output / "process.json", record)
            process.stdin.write(json.dumps(command) + "\n")
            process.stdin.flush()
            reader = threading.Thread(target=lambda: messages.put(process.stdout.readline()), daemon=True)
            reader.start()
            try:
                message = messages.get(timeout=timeout_seconds)
                if not message:
                    raise ProcessStillRunning("supervisor exited without a result handshake")
                record.update(json.loads(message))
            except queue.Empty:
                record["timed_out"] = True
                if (output / "process.json").exists():
                    partial = json.loads((output / "process.json").read_text(encoding="utf-8"))
                    record.update({key: value for key, value in partial.items()
                                   if key not in {"timed_out", "tree_quiescent"}})
            except BaseException as error:
                interrupted = error
                record["interrupted"] = type(error).__name__
            finally:
                try:
                    record["orphaned_descendants"] = owner.stop()
                    record["tree_quiescent"] = True
                except (OSError, subprocess.SubprocessError, ProcessStillRunning) as error:
                    record["cleanup_error"] = str(error)
                    raise ProcessStillRunning("owned tree cleanup could not be proved") from error
    finally:
        record["ownership_observations"] = owner.observations
        owner.close()
        if process:
            # A failed attachment/cleanup may leave our waiting helper. Its
            # retained handle can be stopped without targeting an unrelated PID.
            if process.poll() is None:
                process.kill()
                process.wait(timeout=STOP_TIMEOUT_SECONDS)
            if process.stdin:
                process.stdin.close()
            if reader:
                reader.join(timeout=STOP_TIMEOUT_SECONDS)
            if process.stdout and (reader is None or not reader.is_alive()):
                process.stdout.close()
        record["supervision_finished_utc"] = datetime.now(timezone.utc).isoformat()
        inputs.write_json(output / "process.json", record)
    if interrupted:
        raise interrupted
    for name in ("stdout", "stderr"):
        path = output / (name + ".log")
        record[name + "_text"] = path.read_text(encoding="utf-8", errors="replace") if path.exists() else ""
    return record
