# Install `just`: https://github.com/casey/just  (brew install just / cargo install just)
# Run `just` with no arguments to list commands.

default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Run all fast tests
test:
    cargo test --workspace

# Lint with warnings as errors
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Generate TypeScript protocol bindings, or verify committed bindings with --check
gen-ts mode="":
    #!/usr/bin/env bash
    set -euo pipefail
    destination="$(pwd)/web/src/protocol"
    output="$(mktemp -d)"
    trap 'rm -rf "$output"' EXIT
    case "{{mode}}" in
      "")
        ;;
      "--check")
        ;;
      *)
        echo "usage: just gen-ts [--check]" >&2
        exit 2
        ;;
    esac
    TS_RS_EXPORT_DIR="$output" TS_RS_LARGE_INT=number \
      cargo test -p protocol --features ts export_bindings --quiet
    if [[ "{{mode}}" == "--check" ]]; then
      diff -ru "$destination" "$output"
    else
      mkdir -p "$destination"
      find "$destination" -maxdepth 1 -type f -name '*.ts' -delete
      cp "$output"/*.ts "$destination"/
    fi

# Everything CI runs. Run this before every commit.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace
    just gen-ts --check
    pnpm -C web check
    pnpm -C web build
    python3 -m unittest discover -s eval -p 'test_*.py'
    bash scripts/tests/run-network-test.sh
    bash scripts/tests/run-roles-test.sh
    bash scripts/tests/install-worker-test.sh
    bash scripts/tests/setup-doc-test.sh
    bash scripts/tests/deployment-plan-test.sh
    bash scripts/tests/package-release-test.sh
    bash scripts/tests/notification-examples-test.sh

# Start the daemon and web UI on this machine's Tailscale IPv4 address
network *args:
    bash scripts/run-network.sh {{args}}

# Start only the worker daemon on this Machine's exact Tailscale address
agent *args:
    bash scripts/run-agent.sh {{args}}

# Start the real-data UI for the installed control, or one selected worker
control *args:
    bash scripts/run-control.sh {{args}}

# Install or upgrade the per-user worker from this checked-out source tree
install-worker *args:
    bash scripts/install-worker.sh install {{args}}

# Uninstall worker programs while retaining config and data by default
uninstall-worker *args:
    bash scripts/install-worker.sh uninstall {{args}}

# Build one verified worker artifact for an exact supported target
package-worker version target output="dist":
    bash scripts/build-worker-release.sh --version {{version}} --target {{target}} --output "$(cd . && pwd)/{{output}}"

# Atomically publish one worker artifact to the private package directory
publish-worker version package_dir artifact:
    bash scripts/publish-worker-release.sh publish --version {{version}} --package-dir {{package_dir}} --artifact {{artifact}}

# Move the stable package pointer to an existing immutable release
rollback-worker version package_dir:
    bash scripts/publish-worker-release.sh rollback --version {{version}} --package-dir {{package_dir}}

# Start the fixture-backed UI explicitly for a local product demonstration
demo:
    VITE_DATA=mock pnpm -C web dev --host 127.0.0.1 --port 5173 --strictPort

# Start the fixture-backed UI with repeatable Settings → Simulation controls
user-test:
    VITE_DATA=mock pnpm -C web dev --host 127.0.0.1 --port 5173 --strictPort

# Slow tests that need a real shell / PTY
test-integration:
    cargo test --workspace -- --ignored

# Type-check and run the fast web tests
web-check:
    pnpm -C web check

# Run the browser smoke test against a daemon with the fake PTY backend
test-e2e:
    pnpm -C web test:e2e

# Run all fast checks and the isolated browser acceptance suite
acceptance: check test-e2e
