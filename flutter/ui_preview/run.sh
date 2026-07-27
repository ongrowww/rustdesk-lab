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
exec flutter run -d chrome
