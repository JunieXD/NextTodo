#!/usr/bin/env bash
# Publishes the pushed commit as a release (docs/deploy.md#构建发布): the server image for amd64 and
# arm64 on GHCR, tagged with the version and `latest`, and the installers on GitHub Releases under
# the tag v<version>, with the release notes from CHANGELOG.md's first section, which is to be this
# version's (docs/design/updates.md#更新日志). Needs `docker login ghcr.io` and `gh` signed in, besides what
# build.sh needs.
set -euo pipefail

REPO=JunieXD/NextTodo
IMAGE=ghcr.io/juniexd/nexttodo

cd "$(dirname "$0")/.."
git fetch --quiet origin
# What is built and released, should another session commit here meanwhile.
commit=$(git rev-parse HEAD)
if [[ $commit != $(git rev-parse origin/main) ]]; then
  echo "当前提交和 origin/main 不同，先推送再发布" >&2
  exit 1
fi
version=0.1.$(git rev-list --count "$commit")
notes=$(awk '/^## /{n++} n==1' CHANGELOG.md)
if [[ ${notes%%$'\n'*} != "## $version · "* ]]; then
  echo "CHANGELOG.md 最上面一节不是 $version，先写好这一版的说明" >&2
  exit 1
fi
deploy/build.sh

docker buildx build --platform linux/amd64,linux/arm64 --provenance=false -t "$IMAGE:$version" -t "$IMAGE:latest" --push target/deploy
gh release create "v$version" target/release-assets/* --repo "$REPO" --target "$commit" --title "NextTodo $version" --notes-file - <<<"${notes#*$'\n'}"
echo "已发布 NextTodo $version"
# The tag GitHub made, here too: .githooks/pre-commit counts from it.
git fetch origin tag "v$version"
