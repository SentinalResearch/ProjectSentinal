#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
command_demo="$repo_root/scripts/examples/notification_command_demo.py"
training_demo="$repo_root/scripts/examples/notification_training_demo.py"
launcher="$repo_root/scripts/run-notification-demo.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

fail() {
  echo "notification examples test: $*" >&2
  exit 1
}

[[ -f "$command_demo" ]] || fail "missing notification_command_demo.py"
[[ -f "$training_demo" ]] || fail "missing notification_training_demo.py"
[[ -f "$launcher" ]] || fail "missing run-notification-demo.sh"

success_output="$(python3 -u "$command_demo" success --delay 0)"
[[ "$success_output" == *"Sentinal notification demo: success"* ]] ||
  fail "success output does not identify the scenario"
[[ "$success_output" == *"completed successfully"* ]] ||
  fail "success output does not report completion"

set +e
failure_output="$(python3 -u "$command_demo" failure --delay 0 2>&1)"
failure_status=$?
set -e
[[ "$failure_status" -eq 7 ]] || fail "failure scenario returned $failure_status instead of 7"
[[ "$failure_output" == *"intentional non-zero exit"* ]] ||
  fail "failure output does not explain the non-zero exit"
[[ "$failure_output" != *"error:"* ]] ||
  fail "failure scenario must not also fabricate an ERROR_DETECTED Event"

waiting_output="$(printf 'y\n' | python3 -u "$command_demo" waiting --delay 0)"
[[ "$waiting_output" == *"Continue? [y/N]"* ]] ||
  fail "waiting output does not contain the recognised prompt"
[[ "$waiting_output" == *"response accepted"* ]] ||
  fail "waiting scenario did not consume the response"

training_output="$(cd "$temp_dir" && python3 -u "$training_demo" --epochs 3 --delay 0)"
[[ "$training_output" == *"Epoch 1/3 loss="* ]] ||
  fail "training output does not contain recognised progress"
[[ "$training_output" == *"Saving checkpoint to artifacts/notification-demo-epoch-2.safetensors"* ]] ||
  fail "training output does not contain a recognised checkpoint"
[[ "$training_output" == *"Epoch 3/3 loss="* ]] ||
  fail "training output did not reach the final epoch"
[[ -f "$temp_dir/artifacts/notification-demo-epoch-2.safetensors" ]] ||
  fail "training scenario announced a checkpoint that was not written"

fake_sentinal="$temp_dir/fake-sentinal"
printf '#!/bin/sh\nprintf "<%%s>\\n" "$@"\n' >"$fake_sentinal"
chmod 700 "$fake_sentinal"
launch_output="$(SENTINAL_BIN="$fake_sentinal" bash "$launcher" waiting)"
[[ "$launch_output" == *"<watch>"* ]] || fail "launcher did not request Watch"
[[ "$launch_output" == *"<Notification waiting>"* ]] || fail "launcher lost the title argument"
[[ "$launch_output" == *"notification_command_demo.py>"* ]] ||
  fail "launcher did not use the Command demo"

set +e
invalid_output="$(SENTINAL_BIN="$fake_sentinal" bash "$launcher" unknown 2>&1)"
invalid_status=$?
set -e
[[ "$invalid_status" -eq 2 ]] || fail "unknown launcher scenario returned $invalid_status"
[[ "$invalid_output" == usage:* ]] || fail "unknown launcher scenario lacks usage guidance"

echo "notification examples test: PASS"
