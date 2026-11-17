#!/usr/bin/env bash
# Builds the iOS app's release, unsigned, as apps/ios/build/NextTodo.ipa (docs/ios.md#构建和发布):
# SideStore signs it on the phone with the user's Apple ID. Takes the web app as `cap sync ios` last
# copied it; `pnpm ios:build` in apps/web builds and copies it first.
set -euo pipefail

cd "$(dirname "$0")"
# Xcode's, without making it the system's developer directory.
export DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}
# The version, as the other apps of the same commit have it (docs/ios.md#更新).
commits=$(git rev-list --count HEAD)
out=build/release
rm -rf "$out" build/NextTodo.ipa
# The packages are public: SwiftPM looks for GitHub's credentials in the netrc file, which needs no
# answer, rather than in the keychain, which may wait on a prompt that never shows.
xcodebuild archive -quiet -project App/App.xcodeproj -scheme App -configuration Release -destination generic/platform=iOS \
  -archivePath "$out/NextTodo.xcarchive" -derivedDataPath "$out/derived" -packageAuthorizationProvider netrc \
  CODE_SIGNING_ALLOWED=NO MARKETING_VERSION="0.1.$commits" CURRENT_PROJECT_VERSION="$commits"
mkdir -p "$out/Payload"
cp -R "$out/NextTodo.xcarchive/Products/Applications/App.app" "$out/Payload/"
(cd "$out" && zip -qr ../NextTodo.ipa Payload)
echo "已构建 build/NextTodo.ipa（0.1.$commits）"
