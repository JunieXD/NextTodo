#!/usr/bin/env bash
# Publishes the website (docs/deploy.md#官网): builds apps/site and commits what it builds on the
# gh-pages branch, which GitHub Pages serves at juniexd.github.io/NextTodo.
set -euo pipefail

cd "$(dirname "$0")/.."
if [[ -n $(git status --porcelain) ]]; then
  echo "工作区有未提交的改动，先提交再发布" >&2
  exit 1
fi
pnpm --filter @nexttodo/site build
dist=apps/site/.vitepress/dist
# Served as it is, not through Jekyll.
touch "$dist/.nojekyll"

git fetch --quiet origin
parent=$(git rev-parse --quiet --verify origin/gh-pages || true)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
index=$tmp/index
GIT_INDEX_FILE=$index GIT_WORK_TREE=$dist git add --all
tree=$(GIT_INDEX_FILE=$index git write-tree)
if [[ -n $parent && $tree == $(git rev-parse "$parent^{tree}") ]]; then
  echo "官网没有变化"
  exit 0
fi
commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "发布官网（$(git rev-parse --short HEAD)）")
git push origin "$commit:refs/heads/gh-pages"
echo "已发布官网"
