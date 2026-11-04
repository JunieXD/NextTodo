#!/usr/bin/env bash
# Builds a release of NextTodo on a Mac (docs/deploy.md#构建发布), into two directories:
#
#   target/deploy           what the server image holds: static Linux server binaries for amd64 and
#                           arm64, the web app, and under its `/app/` the apps' updates
#   target/release-assets   the installers to download: the Android APK, the macOS dmg and the
#                           Windows setup
#
# The apps are signed with keys given in the environment, never in the repository:
# NEXTTODO_KEYSTORE and NEXTTODO_KEYSTORE_PASSWORD (docs/android.md#构建和签名),
# TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD (docs/desktop.md#更新).
set -euo pipefail

cd "$(dirname "$0")/.."
if [[ -n $(git status --porcelain) ]]; then
  echo "工作区有未提交的改动，先提交再构建" >&2
  exit 1
fi
for key in NEXTTODO_KEYSTORE NEXTTODO_KEYSTORE_PASSWORD TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD; do
  if [[ -z ${!key:-} ]]; then
    echo "缺少环境变量 $key：App 要用它签名" >&2
    exit 1
  fi
done
version=0.1.$(git rev-list --count HEAD)
# cargo-xwin builds the Windows app with clang-cl and llvm-rc, which Homebrew's llvm has but does not
# put on the PATH.
PATH="$(brew --prefix llvm)/bin:$PATH"
tauri="{\"version\":\"$version\",\"build\":{\"beforeBuildCommand\":\"\"}}"

pnpm install --frozen-lockfile
(cd apps/web && pnpm exec tsc && pnpm exec vite build)
# The apps carry this build of the web app, as it is before compressing it for the server.
(cd apps/web && pnpm exec cap sync android)
(cd apps/android && ./gradlew --quiet assembleRelease)
# CI keeps the dmg script from laying out the window in Finder, which needs a desktop session.
(cd apps/desktop && CI=true pnpm tauri build --bundles app,dmg --config "$tauri")
(cd apps/desktop && pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config "$tauri")
(cd apps/web && node scripts/compress.js)
for arch in x86_64 aarch64; do
  cargo zigbuild --release --locked -p nexttodo-server --target "$arch-unknown-linux-musl"
done

stage=target/deploy
assets=target/release-assets
rm -rf "$stage" "$assets"
mkdir -p "$stage" "$assets"
cp deploy/Dockerfile "$stage/"
cp target/x86_64-unknown-linux-musl/release/nexttodo-server "$stage/nexttodo-server-amd64"
cp target/aarch64-unknown-linux-musl/release/nexttodo-server "$stage/nexttodo-server-arm64"
cp -R apps/web/dist "$stage/web"

# The apps' updates, under `/app/` next to the web app's `web.json` (docs/design.md#app-更新).
app=$stage/web/app
apk=apps/android/app/build/outputs/apk/release/app-release.apk
cp "$apk" "$app/NextTodo.apk"
jq -n --argjson native "$(jq .native apps/web/dist/app/web.json)" --arg version "$version" \
  '{native: $native, versionName: $version, url: "app/NextTodo.apk"}' > "$app/android.json"
bundle=apps/desktop/src-tauri/target/release/bundle
windows=apps/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/NextTodo_${version}_x64-setup.exe
cp "$bundle/macos/NextTodo.app.tar.gz" "$app/"
cp "$windows" "$app/NextTodo-setup.exe"
# Each read on its own line, where set -e stops at a missing signature; inside jq's arguments it
# would not. The downloads are relative to desktop.json: the app resolves them against its server.
mac_signature=$(cat "$bundle/macos/NextTodo.app.tar.gz.sig")
windows_signature=$(cat "$windows.sig")
jq -n --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg mac "$mac_signature" --arg windows "$windows_signature" \
  '{version: $version, pub_date: $date, platforms: {
    "darwin-aarch64": {signature: $mac, url: "NextTodo.app.tar.gz"},
    "windows-x86_64": {signature: $windows, url: "NextTodo-setup.exe"}
  }}' > "$app/desktop.json"

cp "$apk" "$assets/NextTodo-$version-android.apk"
cp "$bundle/dmg/NextTodo_${version}_aarch64.dmg" "$assets/NextTodo-$version-macos-arm64.dmg"
cp "$windows" "$assets/NextTodo-$version-windows-x64-setup.exe"
(cd "$assets" && shasum -a 256 NextTodo-* > SHA256SUMS)
echo "已构建 NextTodo $version：$stage、$assets"
