#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?}" "${GH_REPO:?}"
tag="v${VERSION}"
expected=(
  "axon-${tag}-aarch64-apple-darwin.tar.gz"
  "axon-${tag}-x86_64-apple-darwin.tar.gz"
  "axon-gui-${tag}-aarch64-apple-darwin.zip"
  "axon-gui-${tag}-x86_64-apple-darwin.zip"
)

# A failed API request must not be mistaken for an absent release.
state=$(gh api "repos/$GH_REPO/releases" --paginate \
  --jq ".[] | select(.tag_name == \"$tag\") | if .draft then \"draft\" else \"published\" end")
case "$state" in
  "") gh release create "$tag" --draft --verify-tag --generate-notes ;;
  draft|published) ;;
  *) echo "unexpected release state: $state" >&2; exit 1 ;;
esac

if [ "$state" != published ]; then
  mkdir -p artifacts
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    gh run download "$GITHUB_RUN_ID" --name "axon-$target" --dir artifacts
  done
  for name in "${expected[@]}"; do
    test -s "artifacts/$name"
  done
  for name in "${expected[@]}"; do
    gh release upload "$tag" "artifacts/$name" --clobber
  done
  gh release edit "$tag" --draft=false --latest
fi

# Always hash the published bytes, including after a partial run is retried.
mkdir published
for name in "${expected[@]}"; do
  gh release download "$tag" --pattern "$name" --dir published
  test -s "published/$name"
done
