#!/usr/bin/env bash
# Builds NextTodo and deploys it to rcvps: the web app, a static Linux server binary, a `scratch`
# image carrying both, loaded on the server over ssh and started with docker compose. Nothing is
# compiled on the server. See docs/design.md#部署.
#
#   deploy/deploy.sh          build and deploy
#   deploy/deploy.sh --build  only build the image here
set -euo pipefail

HOST=rcvps
DIR=/opt/1panel/www/sites/NextTodo
PORT=${NEXTTODO_PORT:-8790}
TARGET=x86_64-unknown-linux-musl

cd "$(dirname "$0")/.."
TAG=$(git rev-parse --short HEAD)
if [[ -n $(git status --porcelain) ]]; then
  echo "工作区有未提交的改动，先提交再部署" >&2
  exit 1
fi

(cd apps/web && pnpm install --frozen-lockfile && pnpm build)
cargo zigbuild --release --locked -p nexttodo-server --target "$TARGET"

stage=target/deploy
rm -rf "$stage"
mkdir -p "$stage"
cp "target/$TARGET/release/nexttodo-server" deploy/Dockerfile "$stage/"
cp -R apps/web/dist "$stage/web"
docker build --platform linux/amd64 --provenance=false -t "nexttodo:$TAG" "$stage"

if [[ ${1:-} == --build ]]; then
  echo "已构建 nexttodo:$TAG"
  exit 0
fi

docker save "nexttodo:$TAG" | gzip | ssh "$HOST" 'gunzip | docker load'
ssh "$HOST" "mkdir -p $DIR/data && chown 65534:65534 $DIR/data"
scp deploy/compose.yaml "$HOST:$DIR/compose.yaml"
ssh "$HOST" "cd $DIR && printf 'NEXTTODO_TAG=%s\nNEXTTODO_PORT=%s\n' '$TAG' '$PORT' > .env && docker compose up -d"
echo "已部署 nexttodo:$TAG 到 $HOST（127.0.0.1:$PORT）"
