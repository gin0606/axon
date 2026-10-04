import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    script = Path(__file__).with_name("update-plugin-versions.py").resolve()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

        def run(*args, success=True):
            result = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True)
            assert (result.returncode == 0) == success, result.stdout + result.stderr
            return result.stdout

        def update(*args, success=True):
            return run(sys.executable, str(script), *args, success=success)

        run("git", "init", "-q")
        manifests = []
        for name, base in (("plugins/one", "0.3.0"), ("plugins/two", "0.3.0"),
                           ("examples/agent-workflow", "0.1.0")):
            pair = []
            for host in (".codex-plugin", ".claude-plugin"):
                manifest = root / name / host / "plugin.json"
                manifest.parent.mkdir(parents=True)
                manifest.write_text(json.dumps({"name": name, "version": base}) + "\n")
                pair.append(manifest)
            manifests.append(pair)
            (root / name / "skill.md").write_text("initial\n")
        paths = [path for pair in manifests for path in pair]

        def contents():
            return [path.read_bytes() for path in paths]

        def stage_versions():
            for pair, base in zip(manifests, ("0.3.0", "0.3.0", "0.1.0")):
                versions = [json.loads(path.read_bytes())["version"] for path in pair]
                assert versions[0] == versions[1]
                assert versions[0].startswith(base + "+plugin.")
            run("git", "add", *(str(path) for path in paths))
            update("--check")
            update()

        run("git", "add", ".")
        initial = contents()
        update("--check", success=False)
        assert contents() == initial
        index_before = run("git", "ls-files", "--stage")
        update(success=False)
        assert run("git", "ls-files", "--stage") == index_before
        generated = contents()
        update(success=False)
        assert contents() == generated
        stage_versions()
        run("git", "-c", "user.name=Test", "-c", "user.email=test@example.com",
            "commit", "--no-verify", "-qm", "Initial plugin versions")

        (root / "README.md").write_text("documentation\n")
        (root / "src").mkdir()
        (root / "src/main.rs").write_text("fn main() {}\n")
        run("git", "add", "README.md", "src/main.rs")
        (root / "plugins/one/untracked.md").write_text("untracked\n")
        update()
        assert contents() == generated

        skill = root / "plugins/one/skill.md"
        skill.write_text("initial\nstaged\n")
        run("git", "add", "plugins/one/skill.md")
        skill.write_text("initial\nstaged\nunstaged\n")
        index_before = run("git", "ls-files", "--stage")
        update(success=False)
        assert run("git", "ls-files", "--stage") == index_before
        assert skill.read_text() == "initial\nstaged\nunstaged\n"
        assert contents()[:2] != generated[:2]
        assert contents()[2:] == generated[2:]
        stage_versions()

        previous = contents()
        index_before = run("git", "ls-files", "--stage")
        for text in ("initial\nstaged\n", "initial\nstaged\nmore unstaged edits\n"):
            skill.write_text(text)
            update()
            assert contents() == previous
            assert run("git", "ls-files", "--stage") == index_before

        for manifest in manifests[0]:
            previous = contents()
            metadata = json.loads(manifest.read_text())
            metadata["description"] = "Manifest-only metadata change"
            manifest.write_text(json.dumps(metadata) + "\n")
            run("git", "add", str(manifest))
            update(success=False)
            for path, before in zip(manifests[0], previous[:2]):
                assert json.loads(path.read_bytes())["version"] != json.loads(before)["version"]
            assert contents()[2:] == previous[2:]
            stage_versions()

        for manifest in manifests[0]:
            skill.write_text(f"next change for {manifest.parent.name}\n")
            run("git", "add", "plugins/one/skill.md")
            metadata = json.loads(manifest.read_text())
            metadata["description"] = "Changed metadata"
            manifest.write_text(json.dumps(metadata))
            before = contents()
            update(success=False)
            assert contents() == before
            run("git", "add", str(manifest))
            update(success=False)
            assert json.loads(manifest.read_text())["version"] != metadata["version"]
            stage_versions()

        for manifest in manifests[0]:
            expected = manifest.read_bytes()
            metadata = json.loads(expected)
            metadata["version"] = "0.3.0+manual"
            manifest.write_text(json.dumps(metadata) + "\n")
            run("git", "add", str(manifest))
            update(success=False)
            assert manifest.read_bytes() == expected
            stage_versions()

        for args in (("mv", "plugins/one/skill.md", "plugins/one/renamed.md"),
                     ("update-index", "--chmod=+x", "plugins/one/renamed.md"),
                     ("rm", "-f", "plugins/one/renamed.md")):
            previous = contents()
            run("git", *args)
            update(success=False)
            assert contents()[:2] != previous[:2]
            assert contents()[2:] == previous[2:]
            stage_versions()

        previous = contents()
        example = root / "examples/agent-workflow/skill.md"
        example.write_text("changed example\n")
        run("git", "add", str(example))
        update(success=False)
        assert contents()[:4] == previous[:4]
        assert contents()[4:] != previous[4:]
        stage_versions()

        run("git", "config", "core.autocrlf", "true")
        manifest = manifests[0][0]
        manifest.write_bytes(manifest.read_bytes().replace(b"\n", b"\r\n"))
        assert not run("git", "diff", "--", str(manifest))
        skill.write_text("changed with CRLF checkout\n")
        run("git", "add", str(skill))
        update(success=False)
        stage_versions()
        skill.write_text("another change\n")
        run("git", "add", str(skill))
        metadata = json.loads(manifest.read_text())
        metadata["description"] = "Unstaged CRLF edit"
        manifest.write_bytes((json.dumps(metadata) + "\n").replace("\n", "\r\n").encode())
        before = contents()
        update(success=False)
        assert contents() == before
        run("git", "restore", str(manifest))

        metadata = json.loads(manifest.read_text())
        metadata["version"] = "0.4.0"
        manifest.write_text(json.dumps(metadata) + "\n")
        run("git", "add", str(manifest))
        before = contents()
        update(success=False)
        assert contents() == before
        run("git", "restore", "--staged", str(manifest))
        run("git", "restore", str(manifest))
        update(success=False)
        stage_versions()

        expected = manifest.read_bytes()
        external = root / "external.json"
        external.write_bytes(expected)
        manifest.unlink()
        manifest.symlink_to(external)
        skill.write_text("change with symlink manifest\n")
        run("git", "add", str(skill))
        update(success=False)
        assert manifest.is_symlink()
        assert external.read_bytes() == expected
        manifest.unlink()
        manifest.write_bytes(expected)
        update(success=False)
        stage_versions()

        for path in manifests[0]:
            metadata = json.loads(path.read_text())
            metadata["version"] = "0.4.0"
            path.write_text(json.dumps(metadata) + "\n")
        run("git", "add", *(str(path) for path in manifests[0]))
        update(success=False)
        versions = [json.loads(path.read_text())["version"] for path in manifests[0]]
        assert versions[0] == versions[1]
        assert versions[0].startswith("0.4.0+plugin.")
        run("git", "add", *(str(path) for path in manifests[0]))
        update("--check")

        blob = run("git", "rev-parse", "HEAD:plugins/one/skill.md").strip()
        subprocess.run(
            ["git", "update-index", "--index-info"], cwd=root, env=env, check=True,
            input=f"0 {'0' * len(blob)}\tplugins/one/skill.md\n"
                  f"100644 {blob} 1\tplugins/one/skill.md\n"
                  f"100644 {blob} 2\tplugins/one/skill.md\n", text=True,
        )
        before = contents()
        update(success=False)
        update("--check", success=False)
        assert contents() == before
    print("Plugin version checks passed.")


if __name__ == "__main__":
    main()
