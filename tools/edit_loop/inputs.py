"""Input identity and isolation for edit-loop experiments; no score assignment."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FROZEN_LEXER = ROOT / "tools/outside_score/measure.py"
LEXER_SPEC = importlib.util.spec_from_file_location("edit_loop_frozen_lexer", FROZEN_LEXER)
LEXER = importlib.util.module_from_spec(LEXER_SPEC)
LEXER_SPEC.loader.exec_module(LEXER)
FULL_SHA = re.compile(r"[0-9a-f]{40}")
SAMPLES = 5
JOBS = 2


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n",
                    encoding="utf-8", newline="\n")


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args], stderr=subprocess.PIPE)


def clean_head(repo, sha):
    if not FULL_SHA.fullmatch(sha) or git(repo, "rev-parse", "HEAD").decode().strip() != sha:
        raise ValueError("an exact full current HEAD SHA is required")
    if git(repo, "status", "--porcelain=v1", "--untracked-files=all").strip():
        raise ValueError("the experiment requires a clean tracked and untracked checkout")


def is_link(path):
    info = path.lstat()
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    )


def regular_path(repo, path):
    repo, path = repo.absolute(), path.absolute()
    if ".." in path.parts or not path.is_relative_to(repo) or path == repo:
        raise ValueError("source path escapes the owned checkout")
    current = path
    while current != repo.parent:
        if is_link(current):
            raise ValueError("symlink/junction source or ancestor is not allowed")
        current = current.parent
    if not path.is_file():
        raise ValueError("source must be a regular file")
    return path.relative_to(repo).as_posix()


def new_paths(repo, worktree, target, output):
    paths = [Path(p).absolute() for p in (repo, worktree, target, output)]
    if any(".." in path.parts for path in paths):
        raise ValueError("experiment paths must not contain unresolved parent components")
    for i, left in enumerate(paths):
        for right in paths[i + 1:]:
            if left.is_relative_to(right) or right.is_relative_to(left):
                raise ValueError("source, worktree, targets and evidence must be disjoint")
    for path in paths[1:]:
        if path.exists() or path.is_symlink():
            raise ValueError(f"owned experiment path must not already exist: {path}")
        for parent in path.parents:
            if parent.exists() and is_link(parent):
                raise ValueError("experiment ancestor is a symlink/junction")


def ranked_crates(repo):
    # Reuse the campaign-frozen definition rather than inventing a faster proxy.
    for path in (repo / "crates").rglob("*.rs"):
        regular_path(repo, path)
    stats, files, *_ = LEXER.measure(str(repo), "app", LEXER.toolkit_pattern("egui,eframe"))
    result = []
    for name, values in sorted(stats.items(), key=lambda item: (-item[1]["prod"], item[0])):
        manifest = repo / "crates" / name / "Cargo.toml"
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["name"]
        candidates = [(lines, path) for lines, path in files
                      if lines > 0 and path.startswith(f"crates/{name}/src/")]
        if not candidates:
            raise ValueError(f"no representative production source for {name}")
        # Largest production module, deterministic path tie-break; not a tiny test fixture.
        count, selected = sorted(candidates, key=lambda item: (-item[0], item[1]))[0]
        result.append({"crate": name, "package": package, "production_lines": values["prod"],
                       "source": selected, "source_production_lines": count})
    if len(result) < 3:
        raise ValueError("the frozen ranking does not contain three crates")
    return result


def command_environment(repo, target, jobs, inherited=None):
    env = dict(os.environ if inherited is None else inherited)
    permitted = {"CARGO_HOME", "CARGO_TERM_COLOR", "RUSTUP_HOME"}
    for name, value in env.items():
        if value and name.startswith(("CARGO_", "RUST")) and name not in permitted:
            raise ValueError(f"undeclared build override: {name}")
        if value and name.startswith("QUANTICK_") and name != "QUANTICK_BUBBLES":
            raise ValueError(f"undeclared application/harness override: {name}")
    changes = ["QUANTICK_BUBBLES"] if "QUANTICK_BUBBLES" in env else []
    env.update(CARGO_TERM_COLOR="never", CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS=str(jobs),
               QUANTICK_BUBBLES=str(repo / "crates/app/config/bubbles.toml"))
    return env, changes


def input_hashes(repo):
    paths = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "tools/outside_score/measure.py",
             "crates/app/config/bubbles.toml", "tools/edit_loop/budgets.json"]
    paths += [path.relative_to(repo).as_posix() for path in (repo / "tools/edit_loop").glob("*.py")]
    paths += [path.relative_to(repo).as_posix() for path in (repo / "crates").glob("*/Cargo.toml")]
    return {path: digest((repo / path).read_bytes()) for path in sorted(paths)}


def protocol_hash(hashes):
    paths = [path for path in hashes if path == "tools/outside_score/measure.py"
             or (path.startswith("tools/edit_loop/") and path.endswith(".py")
                 and not Path(path).name.startswith("test_"))]
    return digest("\n".join(f"{path} {hashes[path]}" for path in sorted(paths)).encode())


def cargo_configuration(repo, inherited=None):
    env = os.environ if inherited is None else inherited
    cargo_home = Path(env.get("CARGO_HOME", Path.home() / ".cargo")).absolute()
    locations = [cargo_home] + [parent / ".cargo" for parent in [repo, *repo.parents]]
    records = []
    for directory in dict.fromkeys(locations):
        for name in ("config", "config.toml"):
            path = directory / name
            if not path.exists():
                continue
            regular_path(directory, path)
            data = path.read_bytes()
            config = tomllib.loads(data.decode("utf-8"))
            # The explicit CARGO_BUILD_JOBS below takes precedence over this one
            # common local setting. Every other override needs reviewed support.
            if set(config) - {"build"} or set(config.get("build", {})) - {"jobs"}:
                raise ValueError(f"undeclared Cargo configuration: {path}")
            records.append({"path": str(path), "sha256": digest(data),
                            "normalized_keys": ["build.jobs"] if config.get("build") else []})
    return {"cargo_home": str(cargo_home), "registry_cache": "shared Cargo home; new compile targets",
            "configs": records}


def host_identity():
    info = {"system": platform.system(), "release": platform.release(),
            "version": platform.version(), "machine": platform.machine(),
            "logical_cpus": os.cpu_count(), "processor": platform.processor(),
            "runner_os": os.environ.get("RUNNER_OS"), "runner_arch": os.environ.get("RUNNER_ARCH"),
            "image_os": os.environ.get("ImageOS"), "image_version": os.environ.get("ImageVersion")}
    if os.name == "nt":
        command = ("$c=Get-CimInstance Win32_ComputerSystem; "
                   "$p=Get-CimInstance Win32_Processor; "
                   "@{memory_bytes=$c.TotalPhysicalMemory; cpu_models=@($p.Name); "
                   "disks=@(Get-CimInstance Win32_DiskDrive | Select-Object Model,Size)} "
                   "| ConvertTo-Json -Depth 4 -Compress")
        raw = subprocess.check_output(["powershell", "-NoProfile", "-Command", command],
                                      text=True, timeout=30)
        info["hardware"] = json.loads(raw)
    else:
        info["hardware"] = {"memory_bytes": os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")}
    info["class"] = "/".join(str(info[key]) for key in
                             ("system", "machine", "logical_cpus", "runner_os", "runner_arch", "image_os"))
    return info


def compiler_processes():
    """Observe compiler/cache contention without collecting command lines or tokens."""
    if os.name == "nt":
        command = ("@(Get-Process -Name cargo,rustc,sccache -ErrorAction SilentlyContinue | "
                   "Select-Object Id,ProcessName,CPU) | ConvertTo-Json -Compress")
        raw = subprocess.check_output(["powershell", "-NoProfile", "-Command", command],
                                      text=True, timeout=30)
        value = json.loads(raw) if raw.strip() else []
        return value if isinstance(value, list) else [value]
    raw = subprocess.check_output(["ps", "-eo", "pid=,comm="], text=True, timeout=30)
    return [{"pid": int(pid), "name": name} for line in raw.splitlines()
            for pid, name in [line.strip().split(None, 1)]
            if name in {"cargo", "rustc", "sccache"}]
