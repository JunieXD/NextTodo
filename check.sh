#!/usr/bin/env bash
# Runs every check, as before each commit (AGENTS.md#开发效率): the Rust workspace, the TypeScript
# types made from it, the web app, and the desktop app's shell, a Cargo workspace of its own.
set -euo pipefail

cd "$(dirname "$0")"
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
# The types go to a directory of their own, to be compared with the repository's: stale ones and ones
# left behind by a type removed both show.
types=$(mktemp -d)
trap 'rm -rf "$types"' EXIT
TS_RS_EXPORT_DIR=$types cargo test --locked
if ! diff -r "$types" apps/web/src/api/types; then
  echo "apps/web/src/api/types 和 Rust 的类型不一致，删掉它后在 apps/web 运行 pnpm gen，再提交" >&2
  exit 1
fi
(cd apps/web && pnpm check)
# The shell builds only with the web app and the command line it carries in place.
[[ -d apps/web/dist ]] || (cd apps/web && pnpm exec vite build)
[[ -e apps/desktop/src-tauri/binaries/nexttodo-$(rustc --print host-tuple) ]] || apps/desktop/cli.sh
cd apps/desktop/src-tauri
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
