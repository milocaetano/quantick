"""Bounded Linux host admission; observation never terminates a process."""
from pathlib import Path
import os
import platform
import time

COMPILERS = {'cargo', 'rustc', 'clippy-driver', 'cc', 'c++', 'gcc', 'g++',
             'clang', 'clang++', 'ld', 'ld.lld', 'lld', 'collect2', 'cc1', 'cc1plus'}
OBSERVATION_SECONDS = 3
# Conservative admission only, fixed before any measurements. This does not
# replace the protocol's independent 5% sample-CV validity requirement.
MATERIAL_CORES = 0.10


def process_stat(text):
    left, right = text.index('('), text.rindex(')')
    fields = text[right + 2:].split()
    return {'pid': int(text[:left]), 'name': text[left + 1:right],
            'ppid': int(fields[1]), 'ticks': int(fields[11]) + int(fields[12]),
            'start': int(fields[19])}


def inventory(proc=Path('/proc')):
    if platform.system() != 'Linux':
        raise RuntimeError('Hosted sampling requires the approved Linux host')
    result = []
    for directory in proc.iterdir():
        if not directory.name.isdigit():
            continue
        try:
            result.append(process_stat((directory / 'stat').read_text()))
        except (FileNotFoundError, ProcessLookupError):
            continue  # A process that exited during the bounded observation.
    return result


def descendants(items, roots):
    """Return creation-bound children observed while their build was alive."""
    selected = {p['pid'] for p in items if (p['pid'], p['start']) in roots}
    while True:
        expanded = selected | {p['pid'] for p in items if p['ppid'] in selected}
        if expanded == selected:
            return {(p['pid'], p['start']) for p in items if p['pid'] in selected}
        selected = expanded


def blockers(before, after, elapsed, ticks_per_second, build_descendants):
    previous = {(p['pid'], p['start']): p for p in before}
    findings = []
    for item in after:
        identity = (item['pid'], item['start'])
        old = previous.get(identity)
        cores = ((item['ticks'] - old['ticks']) / ticks_per_second / elapsed
                 if old is not None else None)
        if item['name'].lower() in COMPILERS:
            findings.append({'reason': 'compiler_or_linker_present', **item})
        elif identity in build_descendants and (cores is None or cores > 0):
            findings.append({'reason': 'active_build_descendant', 'cores': cores, **item})
        elif cores is not None and cores > MATERIAL_CORES:
            findings.append({'reason': 'material_background_load', 'cores': cores, **item})
        elif old is None and item['ticks'] > 0:
            findings.append({'reason': 'new_active_process_during_observation', **item})
    return findings


def observe(build_descendants):
    before = inventory()
    started = time.monotonic()
    time.sleep(OBSERVATION_SECONDS)
    after = inventory()
    elapsed = time.monotonic() - started
    return {'before': before, 'after': after, 'elapsed_seconds': elapsed,
            'material_cores_limit': MATERIAL_CORES,
            'coverage': 'Bounded /proc snapshots and CPU deltas cannot exclude future VM scheduling noise.',
            'blockers': blockers(before, after, elapsed, os.sysconf('SC_CLK_TCK'), build_descendants)}


def system_identity():
    cpu = Path('/proc/cpuinfo').read_text()
    policies = {}
    for path in Path('/sys/devices/system/cpu').glob('cpu[0-9]*/cpufreq/scaling_governor'):
        policies[str(path)] = path.read_text().strip()
    return {'platform': platform.platform(), 'cpuinfo': cpu,
            'logical_cpus': os.cpu_count(), 'power_frequency_policy': policies or 'unavailable on host',
            'runner_image': {key: os.environ.get(key) for key in ['ImageOS', 'ImageVersion', 'RUNNER_ARCH']}}
