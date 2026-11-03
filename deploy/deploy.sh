#!/usr/bin/env bash
# Builds NextTodo and deploys it to rcvps: the web app, a static Linux server binary, a `scratch`
# image carrying both, loaded on the server over ssh and started with docker compose. Nothing is
# compiled on the server. See docs/design.md#部署.
#
# The apps' updates go along, under the web app's `/app/` (docs/design.md#app-更新): the release
# APK and the macOS update, signed with keys given in the environment, never in the repository:
# NEXTTODO_KEYSTORE and NEXTTODO_KEYSTORE_PASSWORD (docs/android.md#构建和签名),
# TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD (docs/macos.md#更新).
#
#   deploy/deploy.sh          build and deploy
#   deploy/deploy.sh --build  only build the image here
set -euo pipefail

HOST=rcvps
DIR=/opt/1panel/www/sites/NextTodo
PORT=${NEXTTODO_PORT:-8790}
# Where the macOS app downloads its update from: its update file needs a full address.
URL=${NEXTTODO_URL:-https://todo.juniexd.cn}
TARGET=x86_64-unknown-linux-musl

cd "$(dirname "$0")/.."
TAG=$(git rev-parse --short HEAD)
if [[ -n $(git status --porcelain) ]]; then
  echo "工作区有未提交的改动，先提交再部署" >&2
  exit 1
fi
for key in NEXTTODO_KEYSTORE NEXTTODO_KEYSTORE_PASSWORD TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD; do
  if [[ -z ${!key:-} ]]; then
    echo "缺少环境变量 $key：App 更新包要用它签名" >&2
    exit 1
  fi
done
version=0.1.$(git rev-list --count HEAD)

pnpm install --frozen-lockfile
(cd apps/web && pnpm exec tsc && pnpm exec vite build)
# Both apps carry this build of the web app, as it is before compressing it for the server.
(cd apps/web && pnpm exec cap sync android)
(cd apps/android && ./gradlew --quiet assembleRelease)
(cd apps/desktop && pnpm tauri build --bundles app --config "{\"version\":\"$version\",\"build\":{\"beforeBuildCommand\":\"\"}}")
(cd apps/web && node scripts/compress.js)
cargo zigbuild --release --locked -p nexttodo-server --target "$TARGET"

stage=target/deploy
rm -rf "$stage"
mkdir -p "$stage"
cp "target/$TARGET/release/nexttodo-server" deploy/Dockerfile "$stage/"
cp -R apps/web/dist "$stage/web"
app=$stage/web/app
cp apps/android/app/build/outputs/apk/release/app-release.apk "$app/NextTodo.apk"
jq -n --argjson native "$(jq .native apps/web/dist/app/web.json)" --arg version "$version" \
  '{native: $native, versionName: $version, url: "app/NextTodo.apk"}' > "$app/android.json"
bundle=$(cargo metadata --no-deps --format-version 1 --manifest-path apps/desktop/src-tauri/Cargo.toml | jq -r .target_directory)/release/bundle/macos
cp "$bundle/NextTodo.app.tar.gz" "$app/"
# Read on its own line, where set -e stops at a missing signature; inside jq's arguments it would not.
signature=$(cat "$bundle/NextTodo.app.tar.gz.sig")
jq -n --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg platform "darwin-$(uname -m | sed s/arm64/aarch64/)" \
  --arg signature "$signature" --arg url "$URL/app/NextTodo.app.tar.gz" \
  '{version: $version, pub_date: $date, platforms: {($platform): {signature: $signature, url: $url}}}' > "$app/macos.json"
docker build --platform linux/amd64 --provenance=false -t "nexttodo:$TAG" "$stage"

if [[ ${1:-} == --build ]]; then
  echo "已构建 nexttodo:$TAG"
  exit 0
fi

docker save "nexttodo:$TAG" | gzip | ssh "$HOST" 'gunzip | docker load'
ssh "$HOST" "mkdir -p $DIR"
scp deploy/compose.yaml "$HOST:$DIR/compose.yaml.new"
ssh "$HOST" bash -s -- "$DIR" "$TAG" "$PORT" < deploy/switch.sh
echo "已部署 nexttodo:$TAG 到 $HOST（127.0.0.1:$PORT）"
