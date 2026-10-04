#!/usr/bin/env python3
"""Exercise release failure boundaries without publishing or contacting GitHub."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import release


SCRIPTS = Path(__file__).resolve().parent


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_tag_requires_manifest_match_and_main_ancestry(self):
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

        def git(*args):
            subprocess.run(["git", *args], cwd=self.root, env=env, check=True, capture_output=True)

        git("init", "-b", "main")
        git("config", "user.name", "Release test")
        git("config", "user.email", "release@example.invalid")
        (self.root / "Cargo.toml").write_text('[package]\nname = "axon"\nversion = "0.1.0"\n')
        git("add", "Cargo.toml")
        git("commit", "-m", "Initial manifest")
        git("update-ref", "refs/remotes/origin/main", "HEAD")

        def validate(tag):
            return subprocess.run(
                ["python3", str(SCRIPTS / "release.py"), "validate-tag", tag],
                cwd=self.root, env=env, capture_output=True, text=True,
            )

        self.assertEqual(validate("v0.1.0").stdout.strip(), "0.1.0")
        self.assertEqual(validate("v0.1.0").returncode, 0)
        for tag in ("v0.2.0", "v01.1.0", "v0.1.0-rc1", "0.1.0"):
            self.assertNotEqual(validate(tag).returncode, 0, tag)
        git("commit", "--allow-empty", "-m", "Unmerged change")
        self.assertNotEqual(validate("v0.1.0").returncode, 0)

    def test_formula_checksums_and_rollback_guard(self):
        formula = self.root / "axon.rb"
        for cpu in ("aarch64", "x86_64"):
            (self.root / f"axon-v0.2.0-{cpu}-apple-darwin.tar.gz").write_bytes(cpu.encode())
        formula.write_text('  version "0.1.9"\n')
        release.write_formula("0.2.0", self.root, formula)
        text = formula.read_text()
        for cpu in ("aarch64", "x86_64"):
            self.assertIn(hashlib.sha256(cpu.encode()).hexdigest(), text)
            self.assertIn(f"{cpu}-apple-darwin.tar.gz", text)
        self.assertIn("depends_on macos: :sequoia", text)
        release.check_formula_version("0.2.0", formula)
        release.check_formula_version("0.10.0", formula)
        with self.assertRaises(ValueError):
            release.write_formula("0.1.9", self.root, formula)
        self.assertEqual(formula.read_text(), text)
        formula.write_text("unrecognized formula")
        with self.assertRaises(ValueError):
            release.check_formula_version("0.2.0", formula)

    def publish(self, state, fail=""):
        gh = self.root / "gh"
        gh.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open("calls.jsonl", "a") as log:
    log.write(json.dumps(args) + "\\n")
if " ".join(args[:2]) == os.environ["FAIL"]:
    sys.exit(1)
if args[0] == "api":
    print(os.environ["STATE"])
elif args[:2] == ["release", "download"]:
    name = args[args.index("--pattern") + 1]
    directory = args[args.index("--dir") + 1]
    pathlib.Path(directory, name).write_bytes(b"published bytes")
''')
        gh.chmod(0o755)
        artifacts = self.root / "artifacts"
        artifacts.mkdir()
        for cpu in ("aarch64", "x86_64"):
            (artifacts / f"axon-v0.1.0-{cpu}-apple-darwin.tar.gz").write_bytes(b"rebuilt bytes")
        result = subprocess.run(
            ["bash", str(SCRIPTS / "publish-release.sh")], cwd=self.root,
            env={**os.environ, "PATH": f"{self.root}:{os.environ['PATH']}",
                 "VERSION": "0.1.0", "GH_REPO": "gin0606/axon", "GITHUB_RUN_ID": "123",
                 "STATE": state, "FAIL": fail},
            capture_output=True, text=True,
        )
        calls = [json.loads(line) for line in (self.root / "calls.jsonl").read_text().splitlines()]
        return result, calls

    def test_published_assets_are_only_downloaded_on_retry(self):
        result, calls = self.publish("published")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call[:2] for call in calls[1:]], [["release", "download"]] * 2)
        release.write_formula("0.1.0", self.root / "published", self.root / "axon.rb")
        self.assertIn(hashlib.sha256(b"published bytes").hexdigest(), (self.root / "axon.rb").read_text())

    def test_api_failure_does_not_create_release(self):
        result, calls = self.publish("", "api repos/gin0606/axon/releases")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)

    def test_upload_failure_does_not_publish(self):
        result, calls = self.publish("draft", "release upload")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[:2] == ["release", "edit"] for call in calls))

    def test_missing_release_is_published_before_download(self):
        result, calls = self.publish("")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call[:2] for call in calls[1:]], [
            ["release", "create"], ["run", "download"], ["run", "download"],
            ["release", "upload"], ["release", "upload"],
            ["release", "edit"], ["release", "download"], ["release", "download"],
        ])


if __name__ == "__main__":
    unittest.main()
