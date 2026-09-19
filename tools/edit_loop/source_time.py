"""Set timestamps on an already opened regular file, never a replaced path."""

import os
import stat

# Win32 access/share/disposition flags; deliberately omit FILE_SHARE_DELETE.
GENERIC_READ = 0x80000000
FILE_WRITE_ATTRIBUTES = 0x100
FILE_SHARE_READ = 0x1
FILE_SHARE_WRITE = 0x2
OPEN_EXISTING = 3
FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000
# FILETIME's 100-ns ticks since 1601 precede the Unix epoch by this offset.
FILETIME_EPOCH_TICKS = 116444736000000000
FILETIME_TICK_NS = 100
DWORD_BITS = 32
DWORD_MASK = (1 << DWORD_BITS) - 1


class MetadataHandle:
    def __init__(self, path):
        self.fd = self.open_windows(path) if os.name == "nt" else os.open(
            path, os.O_RDONLY | os.O_NOFOLLOW)
        if not stat.S_ISREG(os.fstat(self.fd).st_mode):
            self.close()
            raise ValueError("timestamp handle is not a regular file")

    @staticmethod
    def open_windows(path):
        import ctypes
        from ctypes import wintypes
        import msvcrt

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        create = kernel.CreateFileW
        create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                           wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
        create.restype = wintypes.HANDLE
        # Read plus attribute writes; share reads/writes, NOT deletion/replacement.
        # OPEN_REPARSE_POINT refuses to follow a link introduced before open.
        handle = create(str(path), GENERIC_READ | FILE_WRITE_ATTRIBUTES,
                        FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING,
                        FILE_FLAG_OPEN_REPARSE_POINT, None)
        if handle == wintypes.HANDLE(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            return msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY)
        except Exception:
            kernel.CloseHandle(wintypes.HANDLE(handle))
            raise

    def set_times(self, atime_ns, mtime_ns):
        if os.name != "nt":
            os.utime(self.fd, ns=(atime_ns, mtime_ns))
            return
        import ctypes
        from ctypes import wintypes
        import msvcrt

        def filetime(value):
            ticks = value // FILETIME_TICK_NS + FILETIME_EPOCH_TICKS
            return wintypes.FILETIME(ticks & DWORD_MASK, ticks >> DWORD_BITS)

        access, modified = filetime(atime_ns), filetime(mtime_ns)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        setter = kernel.SetFileTime
        setter.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.FILETIME),
                           ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME)]
        setter.restype = wintypes.BOOL
        if not setter(msvcrt.get_osfhandle(self.fd), None, ctypes.byref(access), ctypes.byref(modified)):
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None
