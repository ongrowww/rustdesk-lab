#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v flutter >/dev/null 2>&1; then
  echo "Flutter wurde nicht gefunden."
  echo "Mit mise: mise install && mise exec -- ./run.sh"
  exit 1
fi

flutter pub get

PREVIEW_URL="http://127.0.0.1:7357"
(
  for _ in {1..120}; do
    if curl --fail --silent --output /dev/null "$PREVIEW_URL"; then
      open -a "Microsoft Edge" "$PREVIEW_URL"
      exit 0
    fi
    sleep 0.5
  done
) &

exec flutter run \
  -d web-server \
  --web-hostname=127.0.0.1 \
  --web-port=7357
