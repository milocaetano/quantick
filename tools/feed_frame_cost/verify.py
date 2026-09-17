"""Verify complete exported source trees, declared overlays and external tools."""
from pathlib import Path
import difflib
import hashlib
import io
import json
import zipfile
from source import validate_archive_path
from lifecycle import sha

HERE = Path(__file__).resolve().parent
ARCHIVES = {
    'control': ('f217fcf3db65dac0fc1cbb98956d9155000522ac', 'ee24f916ab893f60a6eac2cd77e908aeed21aabd', '09f57769899a90cd7b1c508ded5d3399ca465e3f3f65d6fecf4ef616d4631ba9'),
    'candidate': ('0e0970ab0d9db99470f5427aeac021ba13c5e4ac', 'ab1a14e61d1718b1ea8351351b70be4967e14966', '0ee5211d5c31b1aeef2e2837f70bc0fc91c9054a6247f973cfc5cb52996233dc'),
}
TOOLS = ['build.py', 'common.rs', 'host.py', 'prepare.py', 'run_pairs.py', 'run_geometry.py', 'source.py', 'lifecycle.py', 'verify.py', 'geometry.rs', 'test_linux_cleanup.py', 'test_negative_paths.py']


def archive_files(data):
    files = {}
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for item in archive.infolist():
            validate_archive_path(item.filename)
            if ((item.external_attr >> 16) & 0o170000) == 0o120000:
                raise ValueError('Archive symlink refused')
            if not item.is_dir():
                if item.filename in files:
                    raise ValueError('Duplicate archive member')
                files[item.filename] = archive.read(item)
    return files


def verify_tree(source, originals, overlays):
    expected = {name: hashlib.sha256(data).hexdigest() for name, data in originals.items()}
    expected.update(overlays)
    actual = {}
    for path in Path(source).rglob('*'):
        if path.is_symlink():
            raise ValueError('Source symlink refused')
        if path.is_file():
            actual[path.relative_to(source).as_posix()] = sha(path)
    if actual != expected:
        raise ValueError('Full source mismatch: ' + json.dumps({
            'missing': sorted(expected.keys() - actual.keys()), 'extra': sorted(actual.keys() - expected.keys()),
            'changed': sorted(k for k in actual.keys() & expected.keys() if actual[k] != expected[k])}))


def verify_exports(root):
    root = Path(root)
    manifest = json.loads((root / 'source-manifest.json').read_text(encoding='utf-8'))
    for name, digest in manifest['tools'].items():
        if sha(HERE / name) != digest:
            raise ValueError('External tool changed: ' + name)
    if set(manifest['tools']) != set(TOOLS):
        raise ValueError('Incomplete tool manifest')
    for side, (commit, tree, digest) in ARCHIVES.items():
        entry = manifest['sides'][side]
        archive = root / (side + '-original-source.zip')
        if (entry['commit'], entry['tree'], sha(archive)) != (commit, tree, digest):
            raise ValueError('Immutable archive identity mismatch')
        originals = archive_files(archive.read_bytes())
        verify_tree(root / side, originals, entry['instrumented'])
        patch = ''.join(''.join(difflib.unified_diff(
            originals.get(name, b'').decode('utf-8').splitlines(True),
            (root / side / name).read_text(encoding='utf-8').splitlines(True), fromfile=name, tofile=name))
            for name in entry['instrumented'])
        patch_path = root / (side + '-instrumentation.patch')
        if patch_path.read_bytes() != patch.encode() or sha(patch_path) != entry['patch_sha256']:
            raise ValueError('Patch/postimage mismatch')
        for name, field in [('Cargo.lock', 'lock_sha256'), ('rust-toolchain.toml', 'toolchain_sha256')]:
            if sha(root / side / name) != entry[field]:
                raise ValueError('Lock/toolchain mismatch')
        if sha(root / side / 'crates/app/src/app/tests/f2_frame_protocol_tests.rs') != manifest['common_sha256']:
            raise ValueError('Common fixture differs')
    return manifest
