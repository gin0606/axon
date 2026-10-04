#!/usr/bin/env python3
"""Validate release tags and render the binary-only Homebrew formula."""

import hashlib
import re
import subprocess
import sys
import tomllib
from pathlib import Path


def version_tuple(version):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError(f"invalid release version: {version}")
    return tuple(map(int, version.split(".")))


def check_formula_version(version, formula):
    requested = version_tuple(version)
    if formula.exists():
        current = re.search(r'^  version "([^"]+)"$', formula.read_text(), re.MULTILINE)
        if current is None:
            raise ValueError("cannot read the current axon formula version")
        if version_tuple(current[1]) > requested:
            raise ValueError(f"tap already contains newer version {current[1]}")


def validate_tag(tag):
    if not tag.startswith("v"):
        raise ValueError("release tag must start with v")
    version = tag[1:]
    version_tuple(version)
    with open("Cargo.toml", "rb") as manifest:
        cargo_version = tomllib.load(manifest)["package"]["version"]
    if version != cargo_version:
        raise ValueError(f"tag {tag} does not match Cargo version {cargo_version}")
    subprocess.run(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], check=True)
    print(version)


def write_formula(version, assets, formula):
    check_formula_version(version, formula)
    checksums = {}
    for cpu in ("aarch64", "x86_64"):
        archive = assets / f"axon-v{version}-{cpu}-apple-darwin.tar.gz"
        if archive.stat().st_size == 0:
            raise ValueError(f"empty archive: {archive}")
        checksums[cpu] = hashlib.sha256(archive.read_bytes()).hexdigest()
    formula.write_text(f'''class Axon < Formula
  desc "Local issue tracker for Issues and Groups"
  homepage "https://github.com/gin0606/axon"
  version "{version}"
  license "MIT"

  depends_on macos: :sequoia

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/gin0606/axon/releases/download/v#{{version}}/axon-v#{{version}}-aarch64-apple-darwin.tar.gz"
      sha256 "{checksums['aarch64']}"
    elsif Hardware::CPU.intel?
      url "https://github.com/gin0606/axon/releases/download/v#{{version}}/axon-v#{{version}}-x86_64-apple-darwin.tar.gz"
      sha256 "{checksums['x86_64']}"
    end
  end

  def install
    bin.install "axon"
  end

  test do
    assert_equal "axon #{{version}}", shell_output("#{{bin}}/axon --version").strip
    system bin/"axon", "init", "trial"
    system bin/"axon", "capture", "--label", "docs", "--title", "Write a guide",
           "-m", "Explain installation and basic usage."
    system bin/"axon", "storage", "check"
  end
end
''')


if __name__ == "__main__":
    match sys.argv[1:]:
        case ["validate-tag", tag]:
            validate_tag(tag)
        case ["check-formula-version", version, formula]:
            check_formula_version(version, Path(formula))
        case ["formula", version, assets, formula]:
            write_formula(version, Path(assets), Path(formula))
        case _:
            sys.exit("usage: release.py validate-tag TAG | check-formula-version VERSION FORMULA | formula VERSION ASSETS FORMULA")
