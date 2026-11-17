#!/bin/bash
# Builds local mode's server (crates/ios) for what Xcode builds, as the app's first build phase, into
# the directory the app is linked against (docs/ios.md#本机模式).
set -euo pipefail

case $PLATFORM_NAME in
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  iphoneos) target=aarch64-apple-ios ;;
  *) echo "error: 不支持的平台 $PLATFORM_NAME" >&2; exit 1 ;;
esac
if [[ $CONFIGURATION == Release ]]; then profile=ios; else profile=dev; fi
out=$([[ $profile == dev ]] && echo debug || echo "$profile")

export PATH="$HOME/.cargo/bin:$PATH"
cd "$SRCROOT/../../.."
# Xcode's SDKROOT is the app's: build scripts, which run on the Mac, would be linked against it.
env -u SDKROOT cargo build --locked --profile "$profile" -p nexttodo-ios --target "$target"
mkdir -p "$TARGET_TEMP_DIR/rust"
cp "target/$target/$out/libnexttodo_ios.a" "$TARGET_TEMP_DIR/rust/"
