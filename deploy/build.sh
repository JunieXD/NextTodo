#!/usr/bin/env bash
# Builds a release of NextTodo on a Mac (docs/deploy.md#构建发布), into two directories:
#
#   target/deploy           what the server image holds: static Linux server binaries for amd64 and
#                           arm64, the web app, and under its `/app/` the apps' updates
#   target/release-assets   what is downloaded: the Android APK, the iOS IPA and SideStore's source for
#                           it, the macOS dmgs for Apple silicon and Intel, the Windows setup, the
#                           command line for macOS, Linux and Windows, and the apps' updates for local
#                           mode
#
# The apps are signed with keys given in the environment, never in the repository:
# NEXTTODO_KEYSTORE and NEXTTODO_KEYSTORE_PASSWORD (docs/android.md#构建和签名),
# TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD (docs/desktop.md#更新). The iOS app
# is not: SideStore signs it on the phone (docs/ios.md#构建和发布).
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
pnpm install --frozen-lockfile
# Before the flags below, which would have the checks build everything again.
./check.sh
version=0.1.$(git rev-list --count HEAD)
# cargo-xwin builds the Windows app with clang-cl and llvm-rc, which Homebrew's llvm has but does not
# put on the PATH.
PATH="$(brew --prefix llvm)/bin:$PATH"
# The web app is built once, below, for all; the command line the desktop app carries, for each.
tauri="{\"version\":\"$version\",\"build\":{\"beforeBuildCommand\":\"./cli.sh\"}}"
# Panic messages carry the paths of the sources: the ones on the build machine, which name its user
# and disks, become neutral ones. cargo-xwin and cargo-zigbuild add their own flags to these.
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$(rustc --print sysroot)=/rustc --remap-path-prefix=$PWD=/nexttodo"
# The command line, which says this version (`nexttodo --version`).
export NEXTTODO_VERSION=$version

(cd apps/web && pnpm exec tsc && pnpm exec vite build)
# The apps carry this build of the web app, as it is before compressing it for the server.
(cd apps/web && pnpm exec cap sync android)
# A release build keeps no Gradle daemon alive after it: left idling, one holds gigabytes for hours.
(cd apps/android && ./gradlew --no-daemon --quiet assembleRelease)
(cd apps/web && pnpm exec cap sync ios)
apps/ios/build.sh
# CI keeps the dmg script from laying out the window in Finder, which needs a desktop session.
(cd apps/desktop && CI=true pnpm tauri build --bundles app,dmg --config "$tauri")
(cd apps/desktop && CI=true pnpm tauri build --target x86_64-apple-darwin --bundles app,dmg --config "$tauri")
(cd apps/desktop && pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config "$tauri")
(cd apps/web && node scripts/compress.js)
for arch in x86_64 aarch64; do
  cargo zigbuild --release --locked -p nexttodo-server --target "$arch-unknown-linux-musl"
done
# The command line for Linux; the desktop builds made those for macOS and Windows.
for arch in x86_64 aarch64; do
  cargo zigbuild --release --locked -p nexttodo-cli --target "$arch-unknown-linux-musl"
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
intel=apps/desktop/src-tauri/target/x86_64-apple-darwin/release/bundle
windows=apps/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/NextTodo_${version}_x64-setup.exe
cp "$bundle/macos/NextTodo.app.tar.gz" "$app/"
cp "$intel/macos/NextTodo.app.tar.gz" "$app/NextTodo-x64.app.tar.gz"
cp "$windows" "$app/NextTodo-setup.exe"
# Each read on its own line, where set -e stops at a missing signature; inside jq's arguments it
# would not. The downloads are relative to desktop.json: the app resolves them against its server.
mac_signature=$(cat "$bundle/macos/NextTodo.app.tar.gz.sig")
intel_signature=$(cat "$intel/macos/NextTodo.app.tar.gz.sig")
windows_signature=$(cat "$windows.sig")
jq -n --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg mac "$mac_signature" --arg intel "$intel_signature" --arg windows "$windows_signature" \
  '{version: $version, pub_date: $date, platforms: {
    "darwin-aarch64": {signature: $mac, url: "NextTodo.app.tar.gz"},
    "darwin-x86_64": {signature: $intel, url: "NextTodo-x64.app.tar.gz"},
    "windows-x86_64": {signature: $windows, url: "NextTodo-setup.exe"}
  }}' > "$app/desktop.json"

# Without the version in their names, so that `releases/latest/download/<name>` always finds them.
cp "$apk" "$assets/NextTodo-android.apk"
cp apps/ios/build/NextTodo.ipa "$assets/NextTodo-ios.ipa"
cp "$bundle/dmg/NextTodo_${version}_aarch64.dmg" "$assets/NextTodo-macos-arm64.dmg"
cp "$intel/dmg/NextTodo_${version}_x64.dmg" "$assets/NextTodo-macos-x64.dmg"
cp "$windows" "$assets/NextTodo-windows-x64-setup.exe"
cp target/aarch64-apple-darwin/release/nexttodo "$assets/nexttodo-macos-arm64"
cp target/x86_64-apple-darwin/release/nexttodo "$assets/nexttodo-macos-x64"
cp target/x86_64-unknown-linux-musl/release/nexttodo "$assets/nexttodo-linux-x64"
cp target/aarch64-unknown-linux-musl/release/nexttodo "$assets/nexttodo-linux-arm64"
cp target/x86_64-pc-windows-msvc/release/nexttodo.exe "$assets/nexttodo-windows-x64.exe"
# The same updates for the apps in local mode, which take the manifests from the latest release
# (docs/local.md#更新): their downloads named in full, in this release, so that a manifest never
# points into another one.
release=https://github.com/JunieXD/NextTodo/releases/download/v$version
cp "$bundle/macos/NextTodo.app.tar.gz" "$assets/"
cp "$intel/macos/NextTodo.app.tar.gz" "$assets/NextTodo-x64.app.tar.gz"
jq --arg url "$release/NextTodo-android.apk" '.url = $url' "$app/android.json" > "$assets/android.json"
jq --arg mac "$release/NextTodo.app.tar.gz" --arg intel "$release/NextTodo-x64.app.tar.gz" --arg windows "$release/NextTodo-windows-x64-setup.exe" \
  '.platforms["darwin-aarch64"].url = $mac | .platforms["darwin-x86_64"].url = $intel | .platforms["windows-x86_64"].url = $windows' "$app/desktop.json" > "$assets/desktop.json"
# SideStore's source for the iOS app, which it also checks for updates (docs/ios.md#更新): this version
# alone, its download named in full. The permissions are read from the app, as SideStore refuses one
# whose permissions differ from its source's.
plist=apps/ios/build/release/NextTodo.xcarchive/Products/Applications/App.app/Info.plist
jq -n --arg version "$version" --arg build "${version##*.}" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg url "$release/NextTodo-ios.ipa" --argjson size "$(stat -f %z "$assets/NextTodo-ios.ipa")" \
  --argjson privacy "$(plutil -convert json -o - "$plist" | jq 'with_entries(select(.key | endswith("UsageDescription")))')" \
  '{name: "NextTodo", identifier: "cn.juniexd.nexttodo.source",
    sourceURL: "https://github.com/JunieXD/NextTodo/releases/latest/download/sidestore.json",
    website: "https://juniexd.github.io/NextTodo/", tintColor: "#2E8E6C",
    apps: [{name: "NextTodo", bundleIdentifier: "cn.juniexd.nexttodo", developerName: "JunieXD",
      subtitle: "理清先后，专注当下", localizedDescription: "以先后关系为核心的待办：事项之间可以设依赖，随时告诉你现在能做什么。",
      iconURL: "https://raw.githubusercontent.com/JunieXD/NextTodo/main/apps/web/public/icon-maskable-512.png",
      tintColor: "#2E8E6C", category: "utilities",
      versions: [{version: $version, buildVersion: $build, date: $date, downloadURL: $url, size: $size, minOSVersion: "16.4"}],
      appPermissions: {entitlements: [], privacy: $privacy}}],
    news: []}' > "$assets/sidestore.json"
(cd "$assets" && shasum -a 256 NextTodo* nexttodo-* > SHA256SUMS)
echo "已构建 NextTodo $version：$stage、$assets"
