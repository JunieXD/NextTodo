#!/usr/bin/env bash
# Builds NextTodo with build.sh and deploys it to a server of your own over ssh: the image is loaded
# there and started with docker compose in NEXTTODO_DIR, next to `data/`, listening on
# 127.0.0.1:NEXTTODO_PORT for the reverse proxy in front. Nothing is compiled on the server. See
# docs/deploy.md#用部署脚本.
#
#   NEXTTODO_HOST=myserver NEXTTODO_DIR=/srv/nexttodo NEXTTODO_PORT=8080 deploy/deploy.sh
set -euo pipefail

for key in NEXTTODO_HOST NEXTTODO_DIR; do
  if [[ -z ${!key:-} ]]; then
    echo "缺少环境变量 $key" >&2
    exit 1
  fi
done
HOST=$NEXTTODO_HOST
DIR=$NEXTTODO_DIR
PORT=${NEXTTODO_PORT:-8080}

cd "$(dirname "$0")/.."
deploy/build.sh
TAG=$(git rev-parse --short HEAD)
case $(ssh "$HOST" uname -m) in
  x86_64) platform=linux/amd64 ;;
  aarch64 | arm64) platform=linux/arm64 ;;
  *) echo "不支持服务器的架构" >&2; exit 1 ;;
esac
docker build --platform "$platform" --provenance=false -t "nexttodo:$TAG" target/deploy

docker save "nexttodo:$TAG" | gzip | ssh "$HOST" 'gunzip | docker load'
ssh "$HOST" "mkdir -p $DIR"
scp deploy/compose.yaml "$HOST:$DIR/compose.yaml.new"
ssh "$HOST" bash -s -- "$DIR" "$TAG" "$PORT" < deploy/switch.sh
echo "已部署 nexttodo:$TAG 到 $HOST（127.0.0.1:$PORT）"
