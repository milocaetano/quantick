"""External draft: two finite Windows phases. No action occurs on import."""
import argparse
from datetime import datetime
import re
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time
import tomllib
import zipfile

import owner_perf_runner01 as owner

ROOT = Path("C:/qperf")
HERE = Path(__file__).resolve().parent
OVERLAY = "crates/app/src/app/tests/drawings_tests.rs"


def safe_environment(original):
    """Allowlist OS runtime/build essentials, never inherit credential variables."""
    allowed = {"SYSTEMROOT", "WINDIR", "PATH", "PATHEXT", "COMSPEC", "TEMP", "TMP",
               "PROCESSOR_ARCHITECTURE", "NUMBER_OF_PROCESSORS", "USERPROFILE",
               "APPDATA", "LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)",
               "PROGRAMDATA", "SYSTEMDRIVE", "HOMEDRIVE", "HOMEPATH"}
    return dict({key: value for key, value in original.items() if key.upper() in allowed}, PYTHONDONTWRITEBYTECODE="1")


def admit(binding, event, phase):
    owner.require(binding["phase"] == phase and binding["decision"].startswith("https://github.com/milocaetano/quantick/issues/480#issuecomment-"), "finite decision missing")
    owner.require(event["event"] == "push" and event["attempt"] == "1", "push attempt1 required")
    owner.require(event["repository"] == "milocaetano/quantick" and event["ref"] == binding["ref"] and event["before"] == binding["before"], "wrong push boundary")
    owner.require(len(binding["before"]) == 40 and "PENDING" not in json.dumps(binding), "unresolved binding")
    owner.require(binding["products"]["B"]["commit"] == owner.BASELINE, "wrong baseline")
    owner.require(binding["products"]["C"]["commit"] != owner.BASELINE, "candidate absent")


def command(args, cwd, env, label):
    directory = ROOT / "receipts" / label
    directory.mkdir(exist_ok=False)
    receipt = {"state": "started", "args": args, "utc_ns": time.time_ns(), "timeout_seconds": 3600}
    owner.save(directory / "closure.json", receipt)
    try:
        result = load_supervisor().run_process(args, cwd, env, directory, 3600)
        receipt["supervision"] = {k: v for k, v in result.items() if k not in ("stdout_text", "stderr_text")}
        require_quiescence(result)
        receipt["closed_log_hashes"] = {name: owner.digest(directory / name) for name in ("stdout.log", "stderr.log", "process.json", "supervisor.log")}
        owner.require(result["exit_code"] == 0 and not result["timed_out"] and result["interrupted"] is None and result["state"] == "finished", "compile incomplete")
        receipt["state"] = "terminal"
    except BaseException as error:
        receipt.update(state="failed", error_type=type(error).__name__)
        raise
    finally:
        owner.save(directory / "closure.json", receipt)
    return directory / "stdout.log"


def load_supervisor():
    closure = HERE / "supervisor_closure"
    owner.verify_files(closure, owner.load(HERE / "supervisor-closure.json"), True)
    sys.path.insert(0, str(closure / "tools/edit_loop"))
    import supervisor
    owner.require(Path(supervisor.__file__).resolve() == (closure / "tools/edit_loop/supervisor.py").resolve(), "unexpected supervisor import")
    return supervisor


def require_quiescence(record):
    owner.require(record.get("tree_quiescent") is True, "owned compile tree is not quiescent")
    observations = record.get("ownership_observations", [])
    owner.require(any(o.get("stage") == "after_quiescence" and o.get("accounting_active") == 0 for o in observations), "owned job zero accounting missing")
    # Phase1 may stop contained postbuild helpers. Their orphan flag remains
    # in the receipt; no edit-loop measurement or collector gate is changed.


def capacity(phase):
    import ctypes
    class Memory(ctypes.Structure):
        _fields_ = [("length", ctypes.c_ulong), ("load", ctypes.c_ulong)] + [(name, ctypes.c_ulonglong) for name in ("total", "available", "page_total", "page_available", "virtual_total", "virtual_available", "extended")]
    memory = Memory()
    memory.length = ctypes.sizeof(memory)
    owner.require(ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(memory)), "RAM identity unavailable")
    disk = shutil.disk_usage(ROOT.parent)
    sizes = {name: sum(p.stat().st_size for p in (ROOT / name).rglob("*") if p.is_file()) for name in ("B-target", "C-target", "cargo-home")}
    with (ROOT / "capacity.jsonl").open("a", encoding="utf-8") as stream:
        stream.write(json.dumps({"phase": phase, "utc_ns": time.time_ns(), "disk_free": disk.free, "disk_total": disk.total, "ram_total": memory.total, "ram_available": memory.available, "occupancy": sizes}) + "\n")


def dispose_intermediates(target, kept, receipt_path, closures):
    owner.require(target.absolute() in (ROOT / "B-target", ROOT / "C-target"), "not a fresh owned side target")
    owned = target.absolute()
    owner.require(ROOT.resolve() == ROOT.absolute() and target.resolve() == owned, "target root redirected")
    owner.require(len(closures) == 2 and all(c["state"] == "terminal" for c in closures), "both side profiles must close")
    for closure in closures:
        require_quiescence(closure["supervision"])
    kept = {Path(path).absolute(): sha for path, sha in kept.items()}
    paths = []
    for path in [target, *target.rglob("*")]:
        owner.require(not path.is_symlink() and not path.is_junction() and path.resolve().is_relative_to(target.resolve()), "unsafe target member")
        if path.is_file():
            paths.append(path.absolute())
    owner.require(set(kept) <= set(paths), "retained artifact absent")
    for path, sha in kept.items():
        owner.require(owner.digest(path) == sha, "retained artifact changed")
    disposal = sorted(str(path) for path in paths if path not in kept)
    record = {"state": "started", "target": str(target), "retained": {str(p): h for p, h in kept.items()}, "planned": disposal, "removed": []}
    owner.save(receipt_path, record)
    try:
        for name in disposal:
            path = Path(name)
            owner.require(ROOT.resolve() == ROOT.absolute() and target.resolve() == owned and path.resolve().is_relative_to(owned) and not path.is_symlink() and not path.is_junction(), "disposal path changed")
            for ancestor in [path, *path.parents]:
                if ancestor == ROOT.parent:
                    break
                owner.require(not ancestor.is_symlink() and not ancestor.is_junction(), "disposal ancestor redirected")
            path.unlink()
            record["removed"].append(name)
        for path, sha in kept.items():
            owner.require(owner.digest(path) == sha, "retained artifact changed after disposal")
        record["state"] = "terminal"
    finally:
        owner.save(receipt_path, record)


def observe_launcher_host():
    """Compile-only evidence; query subprocess is bounded only by the job cap."""
    import launcher
    path = ROOT / "launcher-host-identity.json"
    receipt = {"state": "started", "scope": "observation only; not collection admission", "observations": []}
    owner.save(path, receipt)
    try:
        previous = None
        for _ in range(2):
            current, parent, rows = launcher.query_identity()
            public = [{k: row.get(k) for k in ("pid", "parent", "name", "created", "image", "image_sha256")} for row in rows]
            receipt["observations"].append({"utc_ns": time.time_ns(), "current": current, "parent": parent, "rows": public})
            owner.save(path, receipt)
            owner.require(current == os.getpid() and parent == os.getppid() and current != parent, "launcher process relationship mismatch")
            owner.require(len(rows) == 2 and {r["pid"] for r in rows} == {current, parent}, "launcher identities ambiguous")
            own = next(r for r in rows if r["pid"] == current)
            ancestor = next(r for r in rows if r["pid"] == parent)
            owner.require(own["parent"] == parent and ancestor["name"].lower() == "pwsh.exe", "not immediate pwsh parent")
            owner.require(isinstance(own["image"], str) and Path(own["image"]).resolve() == Path(sys.executable).resolve(), "Python executable mismatch")
            for row in rows:
                owner.require(isinstance(row["image"], str) and Path(row["image"]).is_absolute(), "launcher executable path unavailable")
                owner.require(isinstance(row["image_sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", row["image_sha256"]), "launcher executable hash unavailable")
            own_time = datetime.fromisoformat(own["created"])
            parent_time = datetime.fromisoformat(ancestor["created"])
            owner.require(own_time.tzinfo is not None and parent_time.tzinfo is not None and parent_time <= own_time, "launcher creation ordering invalid")
            identity = sorted((r["pid"], r["parent"], r["name"], r["created"], str(Path(r["image"]).resolve()), r["image_sha256"]) for r in rows)
            owner.require(previous is None or identity == previous, "launcher identity changed between observations")
            previous = identity
        receipt["state"] = "terminal"
    except BaseException as error:
        receipt.update(state="failed", error_type=type(error).__name__)
        if isinstance(error, owner.InvalidEvidence):
            receipt["reason"] = str(error)
        raise
    finally:
        owner.save(path, receipt)


def runtime_identity():
    system = Path(os.environ["SYSTEMROOT"]) / "System32"
    return {"system": platform.system(), "version": platform.version(), "machine": platform.machine(),
            "processor": platform.processor(), "logical_cpus": os.cpu_count(),
            "python": platform.python_version(), "image_os": os.environ.get("ImageOS"), "image_version": os.environ.get("ImageVersion"),
            "runtime_dlls": {name: owner.digest(system / name) for name in ("ucrtbase.dll", "vcruntime140.dll", "msvcp140.dll")}}


def source_prepare(repo, subject, label, env):
    archive = ROOT / (label + "-source.zip")
    archive_env = dict(env, TZ="UTC")
    raw = subprocess.check_output(["git", "archive", "--format=zip", subject["commit"]], cwd=repo, env=archive_env)
    archive.write_bytes(raw)
    owner.verify_zip_archive(archive, subject["archive_sha256"], subject["files"])
    source = ROOT / (label + "-source")
    source.mkdir()
    with zipfile.ZipFile(archive) as z:
        for name in subject["files"]:
            path = owner.bound_path(source, name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(z.read(name))
    suffix = (HERE / "owner-action-measurement-suffix02.rs").read_bytes()
    owner.require(hashlib.sha256(suffix).hexdigest() == owner.SUFFIX, "suffix changed")
    path = source / OVERLAY
    path.write_bytes(path.read_bytes() + b"\n" + suffix)
    postimage = dict(subject["files"])
    postimage[OVERLAY] = owner.digest(path)
    owner.verify_files(source, postimage, True)
    return source, archive, postimage


def cargo_configs(source, cargo_home):
    paths = [(f"ancestor-{i}/{name}", parent / ".cargo" / name) for i, parent in enumerate([source, *source.parents]) for name in ("config", "config.toml")]
    paths += [("cargo-home/" + name, cargo_home / name) for name in ("config", "config.toml")]
    return {name: owner.digest(path) for name, path in paths if path.is_file()}


def compile_pair(binding, repo, env):
    manifests = {}
    for label, subject in binding["products"].items():
        owner.require(label in ("B", "C"), "unknown subject")
        source, archive, files = source_prepare(repo, subject, label, env)
        target = ROOT / (label + "-target")
        cargo_home = ROOT / "cargo-home"
        build_env = dict(env, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="1", CARGO_HOME=str(cargo_home))
        compiler = subprocess.check_output(["rustc", "-vV"], cwd=source, env=build_env).decode("utf-8")
        cargo = subprocess.check_output(["cargo", "-vV"], cwd=source, env=build_env).decode("utf-8")
        workspace = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
        lock = tomllib.loads((source / "Cargo.lock").read_text(encoding="utf-8"))
        dependencies = sorted((p["name"], p["version"], p["source"], p.get("checksum")) for p in lock["package"] if "source" in p)
        reports = {"toolchain": {"rustc": compiler, "cargo": cargo, "pin_sha256": owner.digest(source / "rust-toolchain.toml")},
                   "effective_config": {"configs": cargo_configs(source, cargo_home), "profiles": workspace.get("profile", {}), "jobs": 1, "features": [], "target": "default-host"},
                   "external_dependencies": dependencies}
        binaries, commands = {}, []
        for profile in ("test", "release"):
            owner.verify_files(source, files, True)
            args = ["cargo", "test", "-p", "quantick-app", "--bin", "quantick-app", "--no-run", "--locked", "-j", "1", "--message-format=json"]
            if profile == "release":
                args.append("--release")
            commands.append(args)
            log = command(args, source, build_env, label + "-" + profile)
            artifacts = []
            for line in log.read_text(encoding="utf-8").splitlines():
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                if row.get("reason") == "compiler-artifact" and row.get("executable") and row.get("profile", {}).get("test"):
                    artifacts.append(row)
            owner.require(len(artifacts) == 1, "test artifact ambiguous")
            effective = owner.ordinary_default_features(source / "crates/app/Cargo.toml", artifacts[0])
            exe = Path(artifacts[0]["executable"])
            owner.require(exe.is_relative_to(target), "artifact outside target")
            binaries[profile] = {"exe": str(exe), "exe_sha256": owner.digest(exe), "pdb": str(exe.with_suffix(".pdb")), "pdb_sha256": owner.digest(exe.with_suffix(".pdb")), "features": effective, "cargo_artifact": artifacts[0], "profile": profile}
            owner.verify_files(source, files, True)
        reports["build_commands"] = commands
        reports["source_review"] = subject["source_review"]
        bound = {}
        for name, data in reports.items():
            path = ROOT / (label + "-" + name + ".json")
            owner.save(path, data)
            bound[name] = {"path": str(path), "sha256": owner.digest(path)}
        manifest = {**bound, "commit": subject["commit"], "source_root": str(source), "archive_path": str(archive), "archive_sha256": owner.digest(archive), "files": files, "product_files": subject["files"],
                    "suffix_path": str(ROOT / "tools" / "owner-action-measurement-suffix02.rs"), "overlay": {"path": OVERLAY, "suffix_sha256": owner.SUFFIX, "separator": "\n"}, "binaries": binaries, "receipt_sources": []}
        manifests[label] = manifest
        owner.save(ROOT / (label + "-manifest.json"), manifest)
        owner.verify_side(manifest, subject["commit"])
        capacity(label + "-both-profiles-before-disposal")
        kept = {binary[k]: binary[k + "_sha256"] for binary in binaries.values() for k in ("exe", "pdb")}
        closures = [owner.load(ROOT / "receipts" / (label + "-" + p) / "closure.json") for p in ("test", "release")]
        dispose_intermediates(target, kept, ROOT / (label + "-disposal.json"), closures)
        capacity(label + "-after-disposal")
    for name in ("toolchain", "effective_config", "external_dependencies"):
        owner.require(manifests["B"][name]["sha256"] == manifests["C"][name]["sha256"], "pair settings differ")
    return manifests


def package(root, destination, names):
    owner.require(not destination.exists(), "package already exists")
    mapping = {name: owner.digest(owner.bound_path(root, name)) for name in sorted(names)}
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED, allowZip64=True) as z:
        contents = {"manifest.json": json.dumps(mapping, sort_keys=True).encode()}
        for name in ["manifest.json", *mapping]:
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            if name == "manifest.json":
                z.writestr(info, contents[name])
            else:
                with owner.bound_path(root, name).open("rb") as source, z.open(info, "w", force_zip64=True) as output:
                    shutil.copyfileobj(source, output, length=1024 * 1024)
    return mapping


def restore(package_path, expected_sha, root):
    owner.require(not root.exists() and owner.digest(package_path) == expected_sha, "restore target exists or package changed")
    with zipfile.ZipFile(package_path) as z:
        names = z.namelist()
        owner.require(len(names) == len(set(names)) and names.count("manifest.json") == 1, "package duplicate manifest/member")
        mapping = owner.strict_json(z.read("manifest.json").decode("utf-8"))
        # Reuse reviewed ZIP path/type validation, including manifest content.
        complete = dict(mapping, **{"manifest.json": hashlib.sha256(z.read("manifest.json")).hexdigest()})
        owner.verify_zip_archive(package_path, expected_sha, complete)
        root.mkdir()
        for name in mapping:
            path = owner.bound_path(root, name)
            path.parent.mkdir(parents=True, exist_ok=True)
            with z.open(name) as source, path.open("xb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)
    owner.verify_files(root, mapping, True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("phase", choices=("compile", "collect"))
    p.add_argument("--binding", type=Path, required=True)
    p.add_argument("--repo", type=Path, required=True)
    p.add_argument("--transport", type=Path)
    args = p.parse_args()
    binding = owner.load(args.binding)
    event = {key: os.environ[env] for key, env in {"event": "GITHUB_EVENT_NAME", "attempt": "GITHUB_RUN_ATTEMPT", "repository": "GITHUB_REPOSITORY", "ref": "GITHUB_REF", "before": "OWNER_PUSH_BEFORE", "after": "GITHUB_SHA", "run": "GITHUB_RUN_ID"}.items()}
    admit(binding, event, args.phase)
    owner.require(sys.platform == "win32" and not ROOT.exists(), "fresh Windows C:/qperf required")
    env = safe_environment(os.environ)
    owner.require(not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=args.repo, env=env), "carrier checkout is dirty")
    owner.require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.repo, env=env).decode().strip() == event["after"], "checkout is not exact event head")
    changes = subprocess.check_output(["git", "diff", "--name-only", binding["products"]["C"]["commit"], event["after"]], cwd=args.repo, env=env).decode().splitlines()
    binding_name = args.binding.resolve().relative_to(args.repo.resolve()).as_posix()
    owner.require(set(changes) == set(binding["carrier_files"]) | {binding_name}, "carrier changes product inputs")
    # The binding cannot hash itself; root reviews and publishes the actual
    # carrier commit before push. Every other carrier file is bound here.
    owner.verify_files(args.repo, binding["carrier_files"])
    for name, sha in binding["tools"].items():
        owner.require(owner.digest(HERE / name) == sha, "tool changed")
    if args.phase == "collect":
        restore(args.transport, binding["package_sha256"], ROOT)
        owner.require(runtime_identity() == owner.load(ROOT / "runtime.json"), "host/runtime prerequisites differ")
        for label in ("B", "C"):
            owner.verify_side(owner.load(ROOT / (label + "-manifest.json")), binding["products"][label]["commit"])
        # Root supplies the exact reviewed owner release; this does not invent
        # dynamic process exceptions or any performance acceptance policy.
        release_path = args.binding.parent / binding["collection_release_file"]
        owner.require(owner.digest(release_path) == binding["collection_release_sha256"], "collection release changed")
        import launcher
        release_path = launcher.derive(release_path, binding["collection_release_sha256"], args.binding.parent / binding["launcher_policy_file"], binding["launcher_policy_sha256"], ROOT / "launcher-proof", event)
        os.environ.clear()
        os.environ.update(env)
        owner.run(release_path, ROOT / "results")
        return
    ROOT.mkdir()
    (ROOT / "receipts").mkdir()
    (ROOT / "tools").mkdir()
    observe_launcher_host()
    capacity("startup")
    for name, sha in binding["tools"].items():
        owner.require(owner.digest(HERE / name) == sha, "tool changed")
        destination = owner.bound_path(ROOT / "tools", name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((HERE / name).read_bytes())
    owner.save(ROOT / "event.json", event)
    owner.save(ROOT / "runtime.json", runtime_identity())
    pair = compile_pair(binding, args.repo, env)
    capacity("before-package")
    names = {p.relative_to(ROOT).as_posix() for p in ROOT.rglob("*") if p.is_file() and not any(part in ("B-target", "C-target", "cargo-home") for part in p.relative_to(ROOT).parts)}
    for manifest in pair.values():
        for binary in manifest["binaries"].values():
            names.update(Path(binary[key]).relative_to(ROOT).as_posix() for key in ("exe", "pdb"))
    package(ROOT, Path("C:/owner-transport/payload.zip"), names)
    capacity("after-package")


if __name__ == "__main__":
    try:
        main()
    except BaseException as error:
        # Workflow upload retains partial files even for admission failure.
        receipt = Path("C:/owner-transport/failure.json")
        receipt.parent.mkdir(exist_ok=True)
        owner.save(receipt, {"state": "failed", "error_type": type(error).__name__, "utc_ns": time.time_ns()})
        raise SystemExit(1) from None
