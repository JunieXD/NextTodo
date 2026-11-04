#!/usr/bin/env bash
# Publishes the pushed commit as a release (docs/deploy.md#构建发布): the server image for amd64 and
# arm64 on GHCR, tagged with the version and `latest`, and the installers on GitHub Releases under
# the tag v<version>, with the release notes from the file given. Needs `docker login ghcr.io` and
# `gh` signed in, besides what build.sh needs.
#
#   deploy/release.sh notes.md
set -euo pipefail

REPO=JunieXD/NextTodo
IMAGE=ghcr.io/juniexd/nexttodo
notes=$(realpath "${1:?要给出版本说明文件}")

cd "$(dirname "$0")/.."
git fetch --quiet origin
if [[ $(git rev-parse HEAD) != $(git rev-parse origin/main) ]]; then
  echo "当前提交和 origin/main 不同，先推送再发布" >&2
  exit 1
fi
deploy/build.sh
version=0.1.$(git rev-list --count HEAD)

docker buildx build --platform linux/amd64,linux/arm64 --provenance=false -t "$IMAGE:$version" -t "$IMAGE:latest" --push target/deploy
gh release create "v$version" target/release-assets/* --repo "$REPO" --target "$(git rev-parse HEAD)" --title "NextTodo $version" --notes-file "$notes"
echo "已发布 NextTodo $version"
