#!/usr/bin/env bash
# Runs every check, as before each commit (AGENTS.md#开发效率): the Rust workspace, the TypeScript
# types made from it, the web app, the desktop app's shell, a Cargo workspace of its own, and the links
# into the docs. They don't depend on one another and run at once; the output of those that fail is shown.
set -euo pipefail

cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

rust() {
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
  # The types go to a directory of their own, to be compared with the repository's: stale ones and
  # ones left behind by a type removed both show.
  TS_RS_EXPORT_DIR=$tmp/types cargo test --locked
  if ! diff -r "$tmp/types" apps/web/src/api/types; then
    echo "apps/web/src/api/types 和 Rust 的类型不一致，删掉它后在 apps/web 运行 pnpm gen，再提交" >&2
    return 1
  fi
}

web() {
  cd apps/web
  pnpm check
}

desktop() {
  # The shell builds only with the web app and the command line it carries in place.
  [[ -d apps/web/dist ]] || (cd apps/web && pnpm exec vite build)
  [[ -e apps/desktop/src-tauri/binaries/nexttodo-$(rustc --print host-tuple) ]] || apps/desktop/cli.sh
  cd apps/desktop/src-tauri
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
  cargo test --locked
}

links() {
  # Not the Android app's native part: a change there raises its native version (docs/android.md#更新),
  # so its links are put right with the next change it has anyway, which .githooks/pre-commit checks.
  node docs/links.mjs -- . ':!apps/android' ':!apps/web/capacitor.config.ts'
}

# Each check in a process group of its own: interrupted, all stop with what they started.
set -m
checks=(rust web desktop links)
pids=()
stop() {
  kill -TERM -- "${pids[@]/#/-}" 2>/dev/null || true
  wait
  exit 130
}
trap stop INT TERM
for check in "${checks[@]}"; do
  "$check" > "$tmp/$check.log" 2>&1 &
  pids+=($!)
done
failed=0
for i in "${!checks[@]}"; do
  if ! wait "${pids[$i]}"; then
    echo "==== ${checks[$i]} 没通过 ====" >&2
    cat "$tmp/${checks[$i]}.log" >&2
    failed=1
  fi
done
exit $failed
