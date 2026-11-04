#!/usr/bin/env bash
# Run on the server by deploy.sh, with the new compose.yaml uploaded next to the running one as
# compose.yaml.new: switches to the new version, and back to the one before when it doesn't answer.
#
#   ssh <host> bash -s -- <directory> <tag> <port> < deploy/switch.sh
set -euo pipefail
cd "$1"

# The container runs as 65534; files restored or created by root would keep it from writing.
mkdir -p data
chown -R 65534:65534 data

printf 'NEXTTODO_IMAGE=nexttodo:%s\nNEXTTODO_PORT=%s\n' "$2" "$3" > .env.new
docker compose --env-file .env.new -f compose.yaml.new config -q
if [[ -f compose.yaml ]]; then
  cp compose.yaml compose.yaml.prev
  # Without a .env the running compose.yaml uses its defaults, as it does with an empty one.
  if [[ -f .env ]]; then cp .env .env.prev; else : > .env.prev; fi
fi
mv compose.yaml.new compose.yaml
mv .env.new .env

# Started, and answering on its port within 30 seconds.
up() {
  docker compose up -d || return 1
  local port deadline=$((SECONDS + 30))
  port=$(sed -n 's/^NEXTTODO_PORT=//p' .env)
  port=${port:-8080} # compose.yaml's default
  while ((SECONDS < deadline)); do
    curl -fsS --max-time 3 -o /dev/null "http://127.0.0.1:$port/" && return 0
    sleep 1
  done
  return 1
}

if up; then
  rm -f compose.yaml.prev .env.prev
  exit 0
fi
docker compose logs --tail 50 || true
if [[ -f .env.prev ]]; then
  echo "新版本没有启动起来，换回上一个版本" >&2
  mv compose.yaml.prev compose.yaml
  mv .env.prev .env
  up || echo "上一个版本也没有启动起来" >&2
fi
exit 1
