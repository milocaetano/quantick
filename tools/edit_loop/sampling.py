"""Owned source-metadata transactions and observable Cargo process samples."""

import math
import os
import re
import statistics
import time

import inputs
from process_owner import ProcessStillRunning
from source_time import MetadataHandle
from supervisor import run_process as run_process

# A full second above the previous completion avoids coarse filesystem ticks.
TOUCH_SEPARATION_NS = 2_000_000_000


class SourceTouch:
    def __init__(self, repo, path, sha, output):
        self.repo, self.path, self.output = repo, path, output
        try:
            self.relative = inputs.regular_path(repo, path)
        except OSError as error:
            raise ValueError("source does not exist as a regular owned file") from error
        if inputs.LEXER.is_test_file(self.relative) or not self.relative.endswith(".rs"):
            raise ValueError("the touched source must be production Rust")
        self.original = path.stat()
        if self.original.st_nlink != 1:
            raise ValueError("hard-linked source is not exclusively owned by the benchmark checkout")
        self.content = path.read_bytes()
        if inputs.git(repo, "show", f"{sha}:{self.relative}") != self.content:
            raise ValueError("source bytes do not match the measured SHA")
        self.record = {"source": self.relative, "sha": sha, "sha256": inputs.digest(self.content),
                       "atime_ns": self.original.st_atime_ns, "mtime_ns": self.original.st_mtime_ns,
                       "mode": self.original.st_mode, "inode": self.original.st_ino,
                       "device": self.original.st_dev, "state": "saved"}
        (output / "source-original.bin").write_bytes(self.content)
        self.save()
        self.handle = MetadataHandle(path)
        opened = os.fstat(self.handle.fd)
        if (opened.st_nlink != 1
                or (opened.st_ino, opened.st_dev) != (self.original.st_ino, self.original.st_dev)):
            self.handle.close()
            raise ValueError("source changed while opening its metadata handle")

    def save(self):
        inputs.write_json(self.output / "recovery.json", self.record)

    def __enter__(self):
        return self

    def touch(self, now_ns=None):
        inputs.regular_path(self.repo, self.path)
        current = self.path.stat()
        if (current.st_nlink != 1
                or (current.st_ino, current.st_dev) != (self.original.st_ino, self.original.st_dev)
                or self.path.read_bytes() != self.content):
            raise ValueError("source changed before touch")
        timestamp = time.time_ns() if now_ns is None else now_ns
        if timestamp <= self.path.stat().st_mtime_ns:
            raise ValueError("touch timestamp did not advance")
        self.handle.set_times(self.original.st_atime_ns, timestamp)
        actual = self.path.stat().st_mtime_ns
        if actual <= self.original.st_mtime_ns:
            raise ValueError("filesystem did not retain a newer timestamp")
        self.record.update(state="touched", touched_mtime_ns=actual)
        self.save()
        return actual

    def __exit__(self, exception_type, *_):
        try:
            if exception_type and issubclass(exception_type, ProcessStillRunning):
                raise ValueError("owned process termination is unproven; retain recovery record")
            inputs.regular_path(self.repo, self.path)
            current = self.path.stat()
            if (current.st_nlink != 1 or self.path.read_bytes() != self.content
                    or current.st_mode != self.original.st_mode
                    or (current.st_ino, current.st_dev) != (self.original.st_ino, self.original.st_dev)):
                raise ValueError("source changed; refusing to overwrite concurrent changes")
            self.handle.set_times(self.original.st_atime_ns, self.original.st_mtime_ns)
            current = self.path.stat()
            if (current.st_atime_ns, current.st_mtime_ns, current.st_mode) != (
                    self.original.st_atime_ns, self.original.st_mtime_ns, self.original.st_mode):
                raise ValueError("source metadata restore did not round-trip")
            self.record["state"] = "restored"
        except (OSError, ValueError):
            self.record["state"] = "restore_refused"
            self.save()
            raise
        finally:
            self.handle.close()
        self.save()


def validate_sample(sample, package, touched):
    elapsed = sample["elapsed_seconds"]
    if type(elapsed) not in (int, float) or not math.isfinite(elapsed) or elapsed < 0:
        raise ValueError("invalid elapsed time")
    if sample.get("tree_quiescent") is not True or sample.get("orphaned_descendants") is not False:
        raise ValueError("explicit owned-tree quiescence without orphaned descendants is required")
    if (type(sample.get("exit_code")) is not int or sample["exit_code"] != 0
            or sample.get("timed_out") is not False or sample.get("interrupted") is not None):
        raise ValueError("Cargo failed or timed out; raw evidence retained")
    if not re.search(r"test result: ok\. [1-9][0-9]* passed; 0 failed;", sample["stdout_text"]):
        raise ValueError("successful executed tests are missing")
    compiled = bool(re.search(r"(?m)^\s*Compiling " + re.escape(package) + r" v\S+", sample["stderr_text"]))
    if touched and not compiled:
        raise ValueError("touched package did not prove recompilation")
    return compiled


def summary(samples):
    values = [sample["elapsed_seconds"] for sample in samples]
    return {"count": len(values), "median_seconds": statistics.median(values),
            "min_seconds": min(values), "max_seconds": max(values)}
