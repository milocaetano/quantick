"""External preparation harness; Linux execution only, no ambient app inputs."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import platform
import signal
import subprocess
import time


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    with Path(path).open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


class Stage:
    def __init__(self, directory, name, **identity):
        self.directory = Path(directory)
        self.name = name
        self.record = dict(identity, started=now(), secondary=[])

    def __enter__(self):
        self.directory.mkdir(parents=True, exist_ok=True)
        write(self.directory / (self.name + '.started.json'), self.record)
        return self

    def __exit__(self, kind, error, traceback):
        self.record.update(finished=now(), status='PASS' if error is None else 'FAIL',
                           primary=None if error is None else {'type': kind.__name__, 'message': str(error)})
        write(self.directory / (self.name + '.terminal.json'), self.record)
        return False


def linux_only():
    if platform.system() != 'Linux':
        raise RuntimeError('Linux-only protocol: refused before child creation')


def decision_url(value):
    prefix = 'https://github.com/milocaetano/quantick/issues/495#issuecomment-'
    if not value.startswith(prefix) or not value[len(prefix):].isdigit():
        raise ValueError('A concrete issue495 decision URL is required; syntax is not authority')


def session_identity(pid):
    text = (Path('/proc') / str(pid) / 'stat').read_text()
    fields = text[text.rindex(')') + 2:].split()
    return {'pid': pid, 'group': int(fields[2]), 'session': int(fields[3]), 'start': int(fields[19])}


def group_members(identity):
    members = []
    for path in Path('/proc').iterdir():
        if not path.name.isdigit():
            continue
        try:
            item = session_identity(int(path.name))
        except (FileNotFoundError, ProcessLookupError):
            continue
        if item['group'] == identity['group'] and item['session'] == identity['session']:
            members.append(item)
    return members


def environment(scratch, *, build=False, rustup_home=None, target=None, mode=None, size=None):
    """Values are never serialized. Only PATH and rustup location come from caller."""
    scratch = Path(scratch).resolve()
    scratch.mkdir(parents=True, exist_ok=False)
    env = {'PATH': os.environ.get('PATH', os.defpath), 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8'}
    for variable, folder in [('HOME', 'home'), ('TMPDIR', 'tmp'), ('XDG_CONFIG_HOME', 'config'),
                             ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache')]:
        path = scratch / folder
        path.mkdir()
        env[variable] = str(path)
    if build:
        if rustup_home is None or target is None:
            raise ValueError('Explicit rustup home and isolated target required')
        cargo_home = scratch / 'cargo'
        cargo_home.mkdir()
        env.update(CARGO_HOME=str(cargo_home), RUSTUP_HOME=str(Path(rustup_home).resolve()),
                   CARGO_BUILD_JOBS='1', CARGO_TARGET_DIR=str(Path(target).resolve()))
    else:
        if mode not in ('whole', 'panel_actual', 'panel_reference') or size not in ('normal', 'narrow'):
            raise ValueError('Fixed runtime mode and size required')
        env.update(F2_FRAME_MODE=mode, F2_FRAME_SIZE=size)
    return env


def run_owned(command, cwd, env, directory, tag, *, timeout, inspect=None, linux=True):
    """linux=False is private unit-test use with a direct harmless child, not a CLI bypass."""
    process = None
    identity = None
    started = time.monotonic()
    observed = set()
    with Stage(directory, tag, command=command, cwd=str(cwd), exit=None,
               environment_names=sorted(env)) as stage:
        stdout = Path(directory) / (tag + '.stdout')
        stderr = Path(directory) / (tag + '.stderr')
        try:
            if linux:
                linux_only()
            with stdout.open('xb') as out, stderr.open('xb') as err:
                process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err,
                                           start_new_session=linux)
                stage.record['pid'] = process.pid
                if linux:
                    identity = session_identity(process.pid)
                    if identity['group'] != process.pid or identity['session'] != process.pid:
                        raise RuntimeError('Owned child did not establish its isolated session')
                    stage.record['owned_session'] = identity
                # WNOWAIT reserves the leader PID even when it exits first.
                # No poll/wait may reap it before owned-session cleanup finishes.
                def running():
                    if linux:
                        return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None
                    return process.poll() is None
                while running():
                    if inspect is not None:
                        import host
                        items = inspect()
                        roots = {(p['pid'], p['start']) for p in items if p['pid'] == process.pid}
                        observed |= host.descendants(items, roots | observed)
                    if time.monotonic() - started > timeout:
                        raise TimeoutError('Owned command deadline exceeded; no retry')
                    time.sleep(0.05)
                if linux:
                    terminal = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT)
                    stage.record['waitid_terminal'] = {'code': terminal.si_code, 'status': terminal.si_status}
                    bad = terminal.si_code != os.CLD_EXITED or terminal.si_status != 0
                else:
                    bad = process.returncode != 0
                if bad:
                    raise RuntimeError('Owned child returned nonzero terminal status')
        finally:
            import sys
            failing = sys.exc_info()[0] is not None
            if process is not None:
                # Every exceptional path keeps the original Popen ownership handle.
                # The isolated session's process group includes compiler descendants.
                try:
                    if linux:
                        # The unreaped leader and matching creation identity make
                        # this group identifier live-owned, never a stale number.
                        current = session_identity(process.pid)
                        if identity is None or current != identity:
                            raise RuntimeError('Owned session identity unavailable; group signal refused')
                        members = group_members(identity)
                        stage.record['cleanup_members'] = members
                        if failing or any(item['pid'] != process.pid for item in members):
                            os.killpg(identity['group'], signal.SIGKILL)
                            stage.record['owned_group_signalled'] = True
                            deadline = time.monotonic() + 30
                            while True:
                                remaining = group_members(identity)
                                live = []
                                for member in remaining:
                                    try:
                                        stat = (Path('/proc') / str(member['pid']) / 'stat').read_text()
                                        state = stat[stat.rindex(')') + 2:].split()[0]
                                    except (FileNotFoundError, ProcessLookupError):
                                        continue
                                    if state not in ('Z', 'X'):
                                        live.append(member)
                                if not live:
                                    stage.record['group_terminal'] = 'No running members; exited zombies may await their parent reaper'
                                    break
                                if time.monotonic() >= deadline:
                                    raise TimeoutError('Owned group still has running members after cleanup')
                                time.sleep(.05)
                        else:
                            stage.record['owned_group_signalled'] = False
                    elif process.poll() is None:
                        process.kill()
                    stage.record['exit'] = process.wait(timeout=30)
                except BaseException as error:
                    stage.record['secondary'].append({'cleanup': type(error).__name__, 'message': str(error)})
                    stage.record['ownership_unresolved'] = True
                    # Direct child is still unreaped and its PID remains reserved.
                    # Refusing group cleanup must not abandon that owned leader.
                    try:
                        process.kill()
                        stage.record['exit'] = process.wait(timeout=30)
                    except BaseException as wait_error:
                        stage.record['secondary'].append({'direct_wait': str(wait_error)})
                    if not failing:
                        raise
            stage.record.update(observed_build_descendants=sorted(observed),
                                stdout_sha256=sha(stdout) if stdout.exists() else None,
                                stderr_sha256=sha(stderr) if stderr.exists() else None)
        return stage.record


def secondary(stage, operation, name):
    """A final check fails a successful stage, but cannot mask its primary failure."""
    import sys
    primary_active = sys.exc_info()[0] is not None
    try:
        operation()
    except BaseException as error:
        if not primary_active:
            raise
        stage.record['secondary'].append({'check': name, 'type': type(error).__name__, 'message': str(error)})
