"""Exercise generator selection and merge recovery in disposable repositories."""
from pathlib import Path
import hashlib
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
CONTROL = "schemas/control/fixture.json"
UI = "docs/control-plane/ui-behaviour-matrix.md"
HOOKS = ".claude/skills/ui-harness/references/hook-registry.md"


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def hashes(root):
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in root.rglob("*") if path.is_file()}


class Scripts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        for name in ("regen.sh", "merge_main.sh"):
            write(self.root / "tools" / name, (ROOT / "tools" / name).read_text(encoding="utf-8"))
        self.bin = Path(self.temp.name) / "bin"
        self.log = Path(self.temp.name) / "calls"
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        REGEN_CALLS=str(self.log), GIT_CONFIG_NOSYSTEM="1")
        # Producers write into the shell's requested staging directory. The
        # failure path emits a partial document, detecting accidental truncation.
        write(self.bin / "cargo", """#!/bin/sh
set -eu
echo "$*" >>"$REGEN_CALLS"
case "$*" in
  *quantick-control-handlers*)
    while [ "$1" != --root ]; do shift; done
    shift
    mkdir -p "$1/schemas/control"
    printf 'control\\n' >"$1/schemas/control/fixture.json"
    [ "${REGEN_FAIL:-}" != control ] || exit 7;;
  *quantick-operability*)
    printf 'ui\\n'
    [ "${REGEN_FAIL:-}" != ui ] || exit 7;;
  *quantick-app*)
    printf 'hooks\\n'
    [ "${REGEN_FAIL:-}" != hooks ] || exit 7;;
  *) exit 9;;
esac
""")
        write(self.bin / "npm", '#!/bin/sh\necho "npm $*" >>"$REGEN_CALLS"\n')
        for path in self.bin.iterdir():
            path.chmod(0o755)

    def run_script(self, name, *args, success=True):
        result = subprocess.run(["sh", f"tools/{name}.sh", *args], cwd=self.root,
                                env=self.env, capture_output=True, text=True)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def calls(self):
        return self.log.read_text() if self.log.exists() else ""

    def test_target_selection_and_oracle_opt_in(self):
        self.run_script("regen", "--write", "control")
        self.assertNotIn("quantick-app", self.calls())
        self.assertNotIn("quantick-operability", self.calls())
        self.log.unlink()
        self.run_script("regen", "--write", "ui")
        self.assertNotIn("quantick-app", self.calls())
        self.assertNotIn("quantick-control-handlers", self.calls())
        self.log.unlink()
        self.run_script("regen", "--write", "all")
        self.assertIn("quantick-app", self.calls())
        self.assertNotIn("npm", self.calls())
        self.run_script("regen", "--check", "pine-oracle", success=False)
        self.assertNotIn("npm", self.calls())
        self.run_script("regen", "--write", "pine-oracle")
        self.assertIn("npm --prefix tools/pine-oracle run regenerate", self.calls())
        self.run_script("regen", "--write", "unknown", success=False)

    def test_check_is_immutable_and_write_is_stable(self):
        self.run_script("regen", "--write", "all")
        first = hashes(self.root)
        self.run_script("regen", "--write", "all")
        self.assertEqual(first, hashes(self.root))
        self.run_script("regen", "--check", "all")
        write(self.root / CONTROL, "stale\n")
        stale = hashes(self.root)
        self.run_script("regen", "--check", "all", success=False)
        self.assertEqual(stale, hashes(self.root))
        self.run_script("regen", "--write", "all")
        self.assertEqual(first, hashes(self.root))

    def test_failed_producer_preserves_every_artifact(self):
        for path in (CONTROL, UI, HOOKS):
            write(self.root / path, "before\n")
        before = hashes(self.root)
        for producer in ("control", "ui", "hooks"):
            self.env["REGEN_FAIL"] = producer
            self.run_script("regen", "--write", "all", success=False)
            self.assertEqual(before, hashes(self.root))

    def git(self, *args, success=True):
        result = subprocess.run(["git", *args], cwd=self.root, env=self.env,
                                capture_output=True, text=True)
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def merge_fixture(self):
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Generator test")
        self.git("config", "user.email", "generator@example.invalid")
        self.git("config", "core.hooksPath", str(Path(self.temp.name) / "no-hooks"))
        self.git("config", "core.autocrlf", "false")
        self.git("config", "commit.gpgsign", "false")
        write(self.root / ".gitattributes", "*.json linguist-generated merge=binary\n"
              + UI + " linguist-generated merge=binary\n"
              + HOOKS + " linguist-generated merge=binary\n")
        write(self.root / CONTROL, "base\n")
        write(self.root / "source.txt", "base\n")
        self.git("add", ".")
        self.git("commit", "-m", "base")
        self.git("checkout", "-b", "feature")
        write(self.root / CONTROL, "ours\n")
        write(self.root / "source.txt", "ours\n")
        self.git("commit", "-am", "ours")
        self.git("checkout", "main")
        write(self.root / CONTROL, "theirs\n")
        write(self.root / "source.txt", "theirs\n")
        self.git("commit", "-am", "theirs")
        self.git("checkout", "feature")
        self.git("remote", "add", "origin", str(self.root))

    def test_merge_conflicts_failure_and_generated_staging(self):
        self.merge_fixture()
        self.run_script("merge_main", success=False)
        self.assertEqual(self.calls(), "", "generated before resolving source conflicts")
        self.assertIn("source.txt", self.git("diff", "--name-only", "--diff-filter=U"))
        write(self.root / "source.txt", "resolved\n")
        self.git("add", "source.txt")
        write(self.root / "unrelated.txt", "keep untracked\n")
        self.env["REGEN_FAIL"] = "hooks"
        self.run_script("merge_main", "--continue", success=False)
        self.assertIn(CONTROL, self.git("diff", "--name-only", "--diff-filter=U"))
        self.assertTrue(self.git("rev-parse", "--verify", "MERGE_HEAD"))
        del self.env["REGEN_FAIL"]
        self.run_script("merge_main", "--continue")
        self.assertEqual((self.root / CONTROL).read_text(), "control\n")
        self.assertEqual(self.git("status", "--porcelain"), "?? unrelated.txt")
        self.assertEqual(len(self.git("rev-list", "--parents", "-n", "1", "HEAD").split()), 3)
        self.assertIn(UI, self.git("ls-files"))
        self.assertIn(HOOKS, self.git("ls-files"))
        self.run_script("merge_main")  # already integrated, no extra merge

    def test_refused_merge_never_regenerates(self):
        self.merge_fixture()
        write(self.root / "source.txt", "unstaged work\n")
        self.run_script("merge_main", success=False)
        self.assertEqual(self.calls(), "")
        self.assertEqual((self.root / "source.txt").read_text(), "unstaged work\n")
        self.assertFalse((self.root / ".git/merge-main-regenerate").exists())

    def test_manually_committed_merge_requires_fresh_artifacts(self):
        self.merge_fixture()
        self.run_script("merge_main", success=False)
        write(self.root / "source.txt", "resolved\n")
        self.git("add", "source.txt", CONTROL)
        self.git("commit", "--no-edit")
        before = hashes(self.root / "schemas")
        self.run_script("merge_main", "--continue", success=False)
        self.assertEqual(before, hashes(self.root / "schemas"))
        self.run_script("regen", "--write", "all")
        self.git("add", CONTROL, UI, HOOKS)
        self.git("commit", "-m", "regenerate")
        self.run_script("merge_main", "--continue")
        self.assertFalse((self.root / ".git/merge-main-regenerate").exists())


def real_generation():
    """Byte-check actual output, including fixtures that must never be rewritten."""
    immutable = [ROOT / "schemas/control/canonical-json-v1-vectors.json"]
    immutable.extend((ROOT / "schemas/control/released").rglob("*.json"))
    before = {str(path): path.read_bytes() for path in immutable}
    for package in ("quantick-control-handlers", "quantick-operability"):
        tree = subprocess.check_output(["cargo", "tree", "-p", package, "--edges", "normal"],
                                       cwd=ROOT, text=True, encoding="utf-8")
        assert "quantick-app " not in tree, "headless generator depends on the app"
    with tempfile.TemporaryDirectory() as directory:
        target = Path(directory)
        command = ["cargo", "run", "-q", "-p", "quantick-control-handlers", "--bin",
                   "regen-control", "--"]
        def run(*args, ok=True):
            result = subprocess.run(command + list(args), cwd=ROOT, capture_output=True, text=True)
            if (result.returncode == 0) != ok:
                raise AssertionError(result.stdout + result.stderr)
            return result
        run("--write", "--root", directory)
        first = hashes(target)
        listed = run("--list").stdout.splitlines()
        assert sorted(first) == sorted(path.replace("/", os.sep) for path in listed)
        for path in listed + [UI, HOOKS]:
            attributes = subprocess.check_output(
                ["git", "check-attr", "linguist-generated", "merge", "--", path],
                cwd=ROOT, text=True)
            assert "linguist-generated: set" in attributes, path
            assert "merge: binary" in attributes, path
        for path in immutable:
            attributes = subprocess.check_output(
                ["git", "check-attr", "linguist-generated", "--", str(path)],
                cwd=ROOT, text=True)
            assert "linguist-generated: set" not in attributes, path
        for path in listed:
            assert (target / path).read_bytes() == (ROOT / path).read_bytes(), path
        run("--write", "--root", directory)
        assert first == hashes(target)
        run("--check", "--root", directory)
        victim = target / listed[0]
        victim.write_bytes(b"stale\n")
        stale = hashes(target)
        run("--check", "--root", directory, ok=False)
        assert stale == hashes(target)
        # A stale temporary file blocks the batch before any artifact replacement.
        blocker = (target / listed[-1]).with_suffix(".regen-control.tmp")
        blocker.write_bytes(b"another writer\n")
        run("--write", "--root", directory, ok=False)
        assert victim.read_bytes() == b"stale\n"
        assert blocker.read_bytes() == b"another writer\n"
        blocker.unlink()
        run("--write", "--root", directory)
        assert first == hashes(target)
    assert before == {str(path): path.read_bytes() for path in immutable}
    print("real generation: exact bytes, path set, repeated writes, immutable checks and failure atomicity PASS")


if __name__ == "__main__":
    if sys.argv[1:] == ["--real"]:
        real_generation()
    else:
        unittest.main()
