#!/usr/bin/env python3
"""Validate release tags and render the binary-only Homebrew formula and the GUI cask."""

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
    """Refuse to roll back a formula or cask in the tap."""
    requested = version_tuple(version)
    if formula.exists():
        current = re.search(r'^  version "([^"]+)"$', formula.read_text(), re.MULTILINE)
        if current is None:
            raise ValueError(f"cannot read the current version of {formula.name}")
        if version_tuple(current[1]) > requested:
            raise ValueError(f"tap already contains newer version {current[1]}")


def checksums(assets, name):
    result = {}
    for cpu in ("aarch64", "x86_64"):
        archive = assets / name(cpu)
        if archive.stat().st_size == 0:
            raise ValueError(f"empty archive: {archive}")
        result[cpu] = hashlib.sha256(archive.read_bytes()).hexdigest()
    return result


def validate_tag(tag):
    if not tag.startswith("v"):
        raise ValueError("release tag must start with v")
    version = tag[1:]
    version_tuple(version)
    # The CLI and the desktop application are published under the same tag.
    for path in ("Cargo.toml", "crates/axon-gui/Cargo.toml"):
        with open(path, "rb") as manifest:
            cargo_version = tomllib.load(manifest)["package"]["version"]
        if version != cargo_version:
            raise ValueError(f"tag {tag} does not match the version {cargo_version} in {path}")
    subprocess.run(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], check=True)
    print(version)


def write_formula(version, assets, formula):
    check_formula_version(version, formula)
    sums = checksums(assets, lambda cpu: f"axon-v{version}-{cpu}-apple-darwin.tar.gz")
    formula.write_text(f'''class Axon < Formula
  desc "Local issue tracker for Issues and Groups"
  homepage "https://github.com/gin0606/axon"
  version "{version}"
  license "MIT"

  depends_on macos: :sequoia

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/gin0606/axon/releases/download/v#{{version}}/axon-v#{{version}}-aarch64-apple-darwin.tar.gz"
      sha256 "{sums['aarch64']}"
    elsif Hardware::CPU.intel?
      url "https://github.com/gin0606/axon/releases/download/v#{{version}}/axon-v#{{version}}-x86_64-apple-darwin.tar.gz"
      sha256 "{sums['x86_64']}"
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


def write_cask(version, assets, cask):
    check_formula_version(version, cask)
    sums = checksums(assets, lambda cpu: f"axon-gui-v{version}-{cpu}-apple-darwin.zip")
    cask.parent.mkdir(parents=True, exist_ok=True)
    cask.write_text(f'''cask "axon-gui" do
  arch arm: "aarch64", intel: "x86_64"

  version "{version}"
  sha256 arm:   "{sums['aarch64']}",
         intel: "{sums['x86_64']}"

  url "https://github.com/gin0606/axon/releases/download/v#{{version}}/axon-gui-v#{{version}}-#{{arch}}-apple-darwin.zip"
  name "Axon"
  desc "Desktop app for browsing Axon issue trackers"
  homepage "https://github.com/gin0606/axon"

  depends_on macos: :sequoia

  app "Axon.app"

  # The app stays open while in use, so do not replace the bundle under it.
  uninstall quit: "me.gin0606.axon"

  zap trash: [
    "~/Library/Application Support/Axon",
    "~/Library/Saved Application State/me.gin0606.axon.savedState",
  ]
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
        case ["cask", version, assets, cask]:
            write_cask(version, Path(assets), Path(cask))
        case _:
            sys.exit("usage: release.py validate-tag TAG | check-formula-version VERSION FORMULA"
                     " | formula VERSION ASSETS FORMULA | cask VERSION ASSETS CASK")
