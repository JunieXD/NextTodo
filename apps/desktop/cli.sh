#!/usr/bin/env bash
# Builds the command line the app carries (docs/desktop.md#命令行) for the platform Tauri builds for,
# into src-tauri/binaries where tauri.macos.conf.json and tauri.windows.conf.json take it from. Tauri
# runs it before building; on its own it builds for this computer.
set -euo pipefail

triple=${TAURI_ENV_TARGET_TRIPLE:-$(rustc --print host-tuple)}
cd "$(dirname "$0")/../.."
if [[ $triple == *-windows-* ]]; then
  cargo xwin build --release --locked -p nexttodo-cli --target "$triple"
  ext=.exe
else
  cargo build --release --locked -p nexttodo-cli --target "$triple"
  ext=
fi
mkdir -p apps/desktop/src-tauri/binaries
cp "target/$triple/release/nexttodo$ext" "apps/desktop/src-tauri/binaries/nexttodo-$triple$ext"
