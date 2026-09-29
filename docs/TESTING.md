# Testing guide

We write the test first, then the code. This file explains which kind of
test to write and where it goes.

## The four kinds of tests

| Kind | What it checks | Where it lives | Runs in CI |
|---|---|---|---|
| **Unit** | One function or type in isolation | `#[cfg(test)] mod tests` at the bottom of the same file | Yes |
| **Contract** | The JSON wire format never changes by accident | `crates/<crate>/tests/contract.rs` + `tests/fixtures/*.json` | Yes |
| **Behaviour** | A scenario across several types (e.g. a session's whole life) | `crates/<crate>/tests/<scenario>.rs` | Yes |
| **Integration** | Real shells, real PTYs, real network | `tests/*_integration.rs`, marked `#[ignore]` | Manually / nightly |

## How to write a good test

Use the **Arrange → Act → Assert** shape and name the test after the
behaviour, not the function:

```rust
#[test]
fn running_session_can_be_interrupted() {
    // Arrange
    let mut session = test_session();
    session.transition(SessionState::Running).unwrap();

    // Act
    let result = session.transition(SessionState::Interrupted);

    // Assert
    assert!(result.is_ok());
    assert_eq!(session.state, SessionState::Interrupted);
}
```

Good test names read like a sentence:
`completed_session_cannot_start_running_again`,
`recipe_render_fails_on_missing_param`.

## Rules

1. **One behaviour per test.** If the name needs "and", split it.
2. **Test the edges.** Empty strings, zero, missing fields, the last valid
   value and the first invalid one.
3. **Test errors, not just success.** Assert on the specific error variant.
4. **No hidden setup.** Use small helper functions like `test_session()`
   defined in the test module, so each test is readable on its own.
5. **Deterministic.** No real clock, no randomness, no network. Pass
   timestamps in explicitly.
6. **Fast.** The whole `cargo test` should stay under a few seconds.

## Sentinal Watch and capture-mode tests

Milestone 1b is safety-sensitive and receives a wider test matrix than an
ordinary feature. Each stage needs all applicable layers below before its
roadmap checkbox can be completed.

| Area | Fast proof | Manual/platform proof |
|---|---|---|
| Capture contract | enum capability unit tests, validation errors, fixture per origin/request | old-client compatibility review |
| Local socket | fake peer credentials, wrong UID rejection, `0600` mode, malformed message recovery | real UID check on Linux and macOS |
| Watch | argv/quoting tests, cwd/title propagation, no-nesting guard, daemon-down error | close terminal and reattach by CLI and phone |
| External hook | fake socket server scripts, nested-session suppression, silent daemon-down path | real zsh/bash/fish on Linux and macOS |
| Auto-wrap | every skip rule, nesting guard, disabled guard, injected timeout | IDE, SSH, scp, and rsync smoke tests |
| Adopt | fake process table, same-user check, unknown exit, sample `/proc` fd data | Linux `/proc` and macOS off-hot-path detection |
| Log tail | append, partial line, truncate, rotate, missing file, bounded buffer | long-running rotating log smoke test |
| Web capabilities | badge text and forbidden-action absence for every capture level | phone viewport and attach-control smoke test |
| Privacy | audit/storage assertions contain metadata but no input payload | inspect SQLite, audit JSONL, and Session logs |

Performance tests use injected transports and clocks for deterministic CI.
The hook benchmark records p50, p95, and maximum overhead over a documented
sample size; p95 must be below 5 ms. Auto-wrap's fail-open behavior is tested
with a controllable deadline and must select the normal shell by 300 ms.
Neither test may add sleeps longer than 100 ms to the fast suite.

Real shell coverage is a release matrix, not “best effort”:

| Platform | zsh | bash | fish | Watch reattach | UID check |
|---|---:|---:|---:|---:|---:|
| macOS | required | required | required | required | required |
| Linux | required | required | required | required | required |

Missing shells may clean-skip on a developer laptop, but a Milestone 1b
release requires recorded results from both platform rows.

## Contract tests (the most important ones)

The protocol is the spine of the system. The daemon, web UI and
intelligence layer all read the same JSON. Contract tests freeze that JSON.

Each fixture in `crates/protocol/tests/fixtures/` is an example message.
The contract test checks two things:

1. The fixture **parses** into the Rust type (old messages still work).
2. Serialising the parsed value gives back **the same JSON** (we did not
   rename or drop a field).

When you intentionally change the protocol:

1. Update the type.
2. Update the fixture by hand so the diff is visible in review.
3. If the change is breaking, bump `PROTOCOL_VERSION`.

## Watcher tests (from Milestone 0.3)

Watchers are tested with **real saved logs**. Save a log in
`crates/watcher/tests/logs/<tool>/<name>.log` and snapshot the events it
produces with `insta`. When a parser changes, review the snapshot diff.

## Running tests

```bash
just test              # all fast tests
just check             # fmt + clippy + tests (run before every commit)
just network --dry-run # validate/inspect the Tailscale development launch
just demo              # explicit local fixture UI (not a backend test)
cargo test -p protocol # one crate
cargo test session     # tests whose name contains "session"
cargo test -- --ignored  # integration tests
```

`just check` also runs `scripts/tests/run-network-test.sh`. The script test
uses an injected Tailscale address and no network: it checks generated TOML,
frontend proxy arguments, port bounds, and rejection of wildcard, LAN, and
out-of-range CGNAT addresses. It also proves the network launcher forces
`VITE_DATA=api`; data-mode and badge tests prove fixtures are opt-in and
labelled.
