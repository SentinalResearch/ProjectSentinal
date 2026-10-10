#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sentinal_bin="${SENTINAL_BIN:-sentinal}"
scenario="${1:-}"

# A launchd-owned daemon does not inherit Terminal.app's permission to read
# macOS privacy-protected folders such as Desktop. Stage the tiny acceptance
# programs in a private runtime directory so Watch can start from any checkout.
umask 077
runtime_dir="${SENTINAL_DEMO_RUNTIME_DIR:-}"
if [[ -z "$runtime_dir" ]]; then
  runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-notification-demo.XXXXXX")"
else
  if [[ -L "$runtime_dir" ]]; then
    echo "sentinal notification demo: runtime path is a symlink: $runtime_dir" >&2
    exit 2
  fi
  mkdir -p "$runtime_dir"
  chmod 700 "$runtime_dir"
fi

case "$scenario" in
  success|failure|waiting)
    install -m 700 \
      "$repo_root/scripts/examples/notification_command_demo.py" \
      "$runtime_dir/notification_command_demo.py"
    cd "$runtime_dir"
    exec "$sentinal_bin" watch -t "Notification ${scenario}" -- \
      python3 -u "$runtime_dir/notification_command_demo.py" "$scenario"
    ;;
  training)
    install -m 700 \
      "$repo_root/scripts/examples/notification_training_demo.py" \
      "$runtime_dir/notification_training_demo.py"
    cd "$runtime_dir"
    exec "$sentinal_bin" watch -t "Notification training" -- \
      python3 -u "$runtime_dir/notification_training_demo.py"
    ;;
  *)
    echo "usage: scripts/run-notification-demo.sh success|failure|waiting|training" >&2
    exit 2
    ;;
esac
