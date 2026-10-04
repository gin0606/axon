import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def git(*args):
    return subprocess.check_output(["git", *args])


def update_versions(check=False):
    packages = {}
    for record in git(
        "ls-files", "--stage", "-z", "--", "plugins/", "examples/agent-workflow/"
    ).split(b"\0"):
        if not record:
            continue
        metadata, filename = record.split(b"\t", 1)
        mode, blob, stage = metadata.decode().split()
        if stage != "0":
            raise SystemExit("Resolve merge conflicts before updating plugin versions.")
        path = os.fsdecode(filename)
        root = "/".join(path.split("/")[:2])
        packages.setdefault(root, []).append([path, mode, blob])

    updates = []
    stale = []
    for root, entries in sorted(packages.items()):
        manifests = []
        for host in (".codex-plugin", ".claude-plugin"):
            manifest = f"{root}/{host}/plugin.json"
            entry = next((entry for entry in entries if entry[0] == manifest), None)
            if entry is None:
                raise SystemExit(f"Missing staged manifest: {manifest}")
            source_blob = entry[2]
            data = json.loads(git("cat-file", "blob", source_blob))
            manifests.append((manifest, source_blob, data))
            without_version = {key: value for key, value in data.items() if key != "version"}
            normalized = json.dumps(without_version, sort_keys=True).encode()
            entry[2] = hashlib.sha256(normalized).hexdigest()
        bases = {data["version"].split("+", 1)[0] for _, _, data in manifests}
        if len(bases) != 1:
            raise SystemExit(f"Stage matching base versions in both manifests for {root}.")
        digest = hashlib.sha256(json.dumps(sorted(entries)).encode()).hexdigest()[:16]
        version = f"{bases.pop()}+plugin.{digest}"
        for manifest, source_blob, data in manifests:
            if data["version"] == version:
                continue
            stale.append(manifest)
            if check:
                continue
            data["version"] = version
            output = (json.dumps(data, ensure_ascii=False, indent=2) + "\n").encode()
            path = Path(manifest)
            if path.is_symlink() or not path.is_file() or (
                path.read_bytes() != output
                and git("hash-object", f"--path={manifest}", "--", manifest).decode().strip()
                != source_blob
            ):
                raise SystemExit(f"Stage or set aside edits to {manifest} before updating versions.")
            if path.read_bytes() != output:
                updates.append((path, output))

    for path, output in updates:
        path.write_bytes(output)
    if stale:
        print("Plugin versions need updating:" if check else "Stage these generated manifests and retry:")
        for manifest in stale:
            print(f"  {manifest}")
        return 1
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate plugin versions from the Git index.")
    parser.add_argument("--check", action="store_true", help="Check staged versions without writing files.")
    args = parser.parse_args()
    raise SystemExit(update_versions(check=args.check))
