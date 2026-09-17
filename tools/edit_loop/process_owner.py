"""Own the benchmark worker before it can start Cargo; stop only that ownership."""

import os
from pathlib import Path
import signal
import subprocess
import time

# Cleanup is outside measured Cargo wall time. A failure to prove quiescence
# within this bound is unsafe to restore, not permission to kill other jobs.
STOP_TIMEOUT_SECONDS = 10
STOP_POLL_SECONDS = 0.02
# Win32 job information classes and policy flags; no breakaway permission.
JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION = 1
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
OWNED_CLEANUP_EXIT_CODE = 1


class ProcessStillRunning(RuntimeError):
    """Owned process quiescence is unproven; source restoration must refuse."""


class OwnedTree:
    def __init__(self):
        self.process = None
        self.job = WindowsJob() if os.name == "nt" else None
        self.observations = []

    @staticmethod
    def options():
        return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {
            "start_new_session": True}

    def attach(self, process):
        if self.job:
            # CPython retains the real process handle, so assignment cannot be
            # redirected to a recycled numeric PID. The worker is waiting for stdin.
            self.job.attach(process._handle)
        elif not Path("/proc").is_dir() or os.getsid(process.pid) != process.pid:
            raise ProcessStillRunning("an inspectable owned Linux session is required")
        self.process = process

    def stop(self):
        if self.job:
            active = self.job.active()
            descendants = active > 1
            # Diagnostics follow the result/timeout, before cleanup. Preserve
            # the accounting decision even if a listed process exits meanwhile.
            self.observations.append({"stage": "before_termination", "accounting_active": active})
            self.observations[-1].update(self.job.snapshot("before_termination"))
            self.job.terminate()
            deadline = time.monotonic() + STOP_TIMEOUT_SECONDS
            while self.job.active():
                if time.monotonic() >= deadline:
                    raise ProcessStillRunning("Windows job still has active processes")
                time.sleep(STOP_POLL_SECONDS)
            self.observations.append({"stage": "after_quiescence", "accounting_active": 0})
            self.observations[-1].update(self.job.snapshot("after_quiescence"))
        else:
            # The helper remains alive after reporting Cargo's result. Holding
            # this session leader avoids signalling a potentially recycled group.
            if self.process.poll() is not None:
                raise ProcessStillRunning("owned session leader exited before tree cleanup")
            members = linux_members(self.process.pid)
            descendants = len(members) > 1
            os.killpg(self.process.pid, signal.SIGKILL)
            deadline = time.monotonic() + STOP_TIMEOUT_SECONDS
            while linux_members(self.process.pid):
                if time.monotonic() >= deadline:
                    raise ProcessStillRunning("owned Linux session is not quiescent")
                time.sleep(STOP_POLL_SECONDS)
            if descendants:
                # A POSIX descendant can escape a session while being stopped.
                # Best-effort group cleanup is not proof of the whole tree.
                self.process.wait(timeout=STOP_TIMEOUT_SECONDS)
                raise ProcessStillRunning("Linux descendants were active at cleanup; tree closure is unproven")
        self.process.wait(timeout=STOP_TIMEOUT_SECONDS)
        return descendants

    def close(self):
        if self.job:
            self.job.close()


def linux_members(session):
    processes = {}
    for path in Path("/proc").glob("[0-9]*/stat"):
        try:
            # comm may contain spaces and parentheses; the final ')' ends it.
            fields = path.read_text().rsplit(")", 1)[1].split()
        except FileNotFoundError:
            continue
        processes[int(path.parent.name)] = (fields[0], int(fields[1]), int(fields[2]), int(fields[3]))
    owned = {session}
    while True:
        expanded = owned | {pid for pid, (_, parent, _, sid) in processes.items()
                            if parent in owned or sid == session}
        if expanded == owned:
            break
        owned = expanded
    members = [pid for pid in owned if pid in processes and processes[pid][0] not in {"Z", "X"}]
    if any(processes[pid][2:] != (session, session) for pid in members):
        raise ProcessStillRunning("owned descendant escaped its original Linux session/group")
    return members


class WindowsJob:
    def __init__(self):
        import ctypes
        from ctypes import wintypes

        self.ctypes = ctypes
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.handle = None
        size = ctypes.c_size_t

        class Limits(ctypes.Structure):
            _fields_ = [("process_time", ctypes.c_longlong), ("job_time", ctypes.c_longlong),
                        ("flags", wintypes.DWORD), ("minimum_working_set", size),
                        ("maximum_working_set", size), ("active_process_limit", wintypes.DWORD),
                        ("affinity", size), ("priority", wintypes.DWORD), ("scheduling", wintypes.DWORD)]

        class Extended(ctypes.Structure):
            _fields_ = [("basic", Limits), ("io", ctypes.c_ulonglong * 6),
                        ("process_memory", size), ("job_memory", size),
                        ("peak_process_memory", size), ("peak_job_memory", size)]

        class Accounting(ctypes.Structure):
            _fields_ = [("user_time", ctypes.c_longlong), ("kernel_time", ctypes.c_longlong),
                        ("period_user_time", ctypes.c_longlong), ("period_kernel_time", ctypes.c_longlong),
                        ("page_faults", wintypes.DWORD), ("total_processes", wintypes.DWORD),
                        ("active_processes", wintypes.DWORD), ("terminated", wintypes.DWORD)]

        self.accounting_type = Accounting
        signatures = {
            "CreateJobObjectW": ([wintypes.LPVOID, wintypes.LPCWSTR], wintypes.HANDLE),
            "SetInformationJobObject": ([wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD], wintypes.BOOL),
            "QueryInformationJobObject": ([wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD, wintypes.LPVOID], wintypes.BOOL),
            "AssignProcessToJobObject": ([wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
            "TerminateJobObject": ([wintypes.HANDLE, wintypes.UINT], wintypes.BOOL),
            "CloseHandle": ([wintypes.HANDLE], wintypes.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.kernel, name)
            function.argtypes, function.restype = arguments, result
        self.handle = self.kernel.CreateJobObjectW(None, None)
        self.check(self.handle)
        limits = Extended()
        limits.basic.flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        try:
            self.check(self.kernel.SetInformationJobObject(
                self.handle, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION, ctypes.byref(limits), ctypes.sizeof(limits)))
        except BaseException:
            self.close()
            raise

    def check(self, result):
        if not result:
            raise ProcessStillRunning(str(self.ctypes.WinError(self.ctypes.get_last_error())))

    def attach(self, handle):
        self.check(self.kernel.AssignProcessToJobObject(self.handle, handle))

    def active(self):
        info = self.accounting_type()
        self.check(self.kernel.QueryInformationJobObject(
            self.handle, JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
            self.ctypes.byref(info), self.ctypes.sizeof(info), None))
        return info.active_processes

    def terminate(self):
        self.check(self.kernel.TerminateJobObject(self.handle, OWNED_CLEANUP_EXIT_CODE))

    def snapshot(self, stage):
        from job_diagnostics import snapshot
        return snapshot(self, stage)

    def close(self):
        if self.handle is not None:
            self.kernel.CloseHandle(self.handle)
            self.handle = None
