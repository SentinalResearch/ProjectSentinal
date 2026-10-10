#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sentinal_bin="${SENTINAL_BIN:-sentinal}"
scenario="${1:-}"

case "$scenario" in
  success|failure|waiting)
    exec "$sentinal_bin" watch -t "Notification ${scenario}" -- \
      python3 -u "$repo_root/scripts/examples/notification_command_demo.py" "$scenario"
    ;;
  training)
    exec "$sentinal_bin" watch -t "Notification training" -- \
      python3 -u "$repo_root/scripts/examples/notification_training_demo.py"
    ;;
  *)
    echo "usage: scripts/run-notification-demo.sh success|failure|waiting|training" >&2
    exit 2
    ;;
esac
