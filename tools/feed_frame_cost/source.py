"""Source-export safety independent of the benchmark runtime."""
from pathlib import PurePosixPath, PureWindowsPath
import io
import stat
import zipfile


def validate_archive_path(name):
    path = PurePosixPath(name)
    windows = PureWindowsPath(name)
    if path.is_absolute() or windows.drive or '\\' in name or '..' in path.parts:
        raise ValueError(f'Unsafe source archive path: {name}')


def extract_archive(archive, destination):
    with zipfile.ZipFile(io.BytesIO(archive)) as stream:
        for entry in stream.infolist():
            validate_archive_path(entry.filename)
            if stat.S_ISLNK(entry.external_attr >> 16):
                raise ValueError(f'Source archive link refused: {entry.filename}')
        destination.mkdir()
        stream.extractall(destination)


def verify_binary_identity(build, executables, hash_file):
    for field in ['profile', 'jobs', 'rustc', 'cargo']:
        if build['control'][field] != build['candidate'][field]:
            raise ValueError(f'Build identity mismatch: {field}')
    for side, path in executables.items():
        if build[side]['binary_sha256'].lower() != hash_file(path):
            raise ValueError(f'Binary hash mismatch: {side}')
        if not build[side]['command'] or build[side]['exit'] != 0:
            raise ValueError(f'Build failed or command missing: {side}')
