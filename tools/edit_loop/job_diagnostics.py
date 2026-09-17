"""Bounded read-only identity snapshots of the already-owned Windows job.

These observations never decide whether a sample passes, extend Cargo's
stopwatch, or authorize a PID-based kill. Membership is verified through a
retained process handle before its image/lifetime is read; a recycled PID is
reported as unresolved, never treated as the original descendant.
"""

from datetime import datetime, timezone

# Win32 job information class and read/synchronization rights, not kill rights.
JOB_OBJECT_BASIC_PROCESS_ID_LIST = 3
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
SYNCHRONIZE = 0x100000
WAIT_FAILED = 0xFFFFFFFF
# Bounded diagnostic allocation. Overflow fails closed instead of truncating.
MAX_JOB_PROCESSES = 4096
MAX_IMAGE_CHARACTERS = 32768


def snapshot(job, stage):
    import ctypes
    from ctypes import wintypes

    class ProcessIds(ctypes.Structure):
        _fields_ = [("assigned", wintypes.DWORD), ("listed", wintypes.DWORD),
                    ("pids", ctypes.c_size_t * MAX_JOB_PROCESSES)]

    kernel = job.kernel
    signatures = {
        "OpenProcess": ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
        "IsProcessInJob": ([wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)], wintypes.BOOL),
        "QueryFullProcessImageNameW": ([wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR,
                                       ctypes.POINTER(wintypes.DWORD)], wintypes.BOOL),
        "GetProcessTimes": ([wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4, wintypes.BOOL),
        "WaitForSingleObject": ([wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(kernel, name)
        function.argtypes, function.restype = arguments, result
    info = ProcessIds()
    job.check(kernel.QueryInformationJobObject(
        job.handle, JOB_OBJECT_BASIC_PROCESS_ID_LIST, ctypes.byref(info), ctypes.sizeof(info), None))
    if info.assigned != info.listed or info.listed > MAX_JOB_PROCESSES:
        raise ValueError("owned job diagnostic process list is incomplete")
    rows = []
    for pid in info.pids[:info.listed]:
        row = {"pid": pid}
        handle = kernel.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, False, pid)
        if not handle:
            # Exit between enumeration and OpenProcess is observable uncertainty,
            # not a reason to clear the separate accounting/orphan decision.
            row["open_error"] = ctypes.get_last_error()
            rows.append(row)
            continue
        try:
            member = wintypes.BOOL()
            job.check(kernel.IsProcessInJob(handle, job.handle, ctypes.byref(member)))
            row["in_owned_job"] = bool(member.value)
            if member.value:
                image = ctypes.create_unicode_buffer(MAX_IMAGE_CHARACTERS)
                size = wintypes.DWORD(len(image))
                job.check(kernel.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(size)))
                row["image"] = image.value
                times = [wintypes.FILETIME() for _ in range(4)]
                job.check(kernel.GetProcessTimes(handle, *(ctypes.byref(value) for value in times)))
                row["creation_filetime"] = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
                row["exit_filetime"] = (times[1].dwHighDateTime << 32) | times[1].dwLowDateTime
                # Raw WAIT_OBJECT_0 / WAIT_TIMEOUT status, not an inferred verdict.
                row["wait_status"] = kernel.WaitForSingleObject(handle, 0)
                job.check(row["wait_status"] != WAIT_FAILED)
        finally:
            kernel.CloseHandle(handle)
        rows.append(row)
    return {"stage": stage, "observed_utc": datetime.now(timezone.utc).isoformat(),
            "assigned": info.assigned, "listed": info.listed, "processes": rows}
