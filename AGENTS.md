# AGENTS.md — Rules for anyone writing code here (AI or human)

Read this file before changing anything. It is short on purpose.

## 1. What this project is

A local-first system that turns terminals on your machines into persistent,
observable, remotely accessible work sessions. Structured state first,
intelligence second.

The layers, bottom to top:

```
protocol   → shared types (the contract). Everything depends on this.
daemon     → PTY sessions, command tracking, metrics (Rust, per machine)
watcher    → turns raw output into Events (deterministic, no AI)
web / cli  → interfaces
intelligence → optional, separate process, uses the same API as the UI
```

**Golden rule:** lower layers never depend on higher layers, and nothing
below `intelligence` may require a model to work.

## 2. Before you write code

1. Find the milestone you are working on in `docs/ROADMAP.md`. Do not build
   things from later milestones "while you are there".
2. If you change a type in `crates/protocol`, you are changing the contract.
   Update the JSON fixtures in `crates/protocol/tests/fixtures/` on purpose,
   and say so in your commit message.
3. Write or update the test first. Then write the code that makes it pass.

## 3. Coding rules

**Keep it simple**
- Prefer plain functions and plain structs. Add a trait only when there are
  two real implementations.
- No new dependency without a one-line reason in the PR/commit. Check the
  approved list in `docs/DEPENDENCIES.md` first.
- One file should do one job. If a file passes ~400 lines, split it.

**Rust specifics**
- `cargo fmt` and `cargo clippy -- -D warnings` must be clean.
- No `unwrap()` / `expect()` in library code. Return a `Result` with a
  `thiserror` error enum. `unwrap()` is fine in tests.
- No `unsafe` unless a module comment explains exactly why.
- Public items get a one-line doc comment saying *what* it is, not how.
- Wire types derive `Serialize, Deserialize, Debug, Clone, PartialEq`.
- Enums on the wire use `SCREAMING_SNAKE_CASE`; struct fields use
  `snake_case`. Timestamps are Unix milliseconds (`Timestamp`).

**Errors and logging**
- Errors say what failed and with which input: `"unknown recipe param 'port'"`
  not `"invalid input"`.
- Use `tracing` for logs (once the daemon exists). Never `println!` in
  library code.

**Naming**
- Use the words from the spec: Machine, Project, Session, Command, Event,
  Recipe, Process, Metric. Do not invent synonyms (`Job`, `Task`, `Run`).

## 4. Testing rules (details in `docs/TESTING.md`)

- Every bug fix comes with a test that fails without the fix.
- Every public function has at least one test for the normal case and one
  for an edge or error case.
- Wire format is protected by **contract tests** (JSON fixtures). If a
  contract test fails, you broke compatibility — fix the code, or bump the
  protocol version and update the fixture deliberately.
- Tests must be fast and offline. No network, no sleeps over 100 ms,
  no reliance on the machine's real shell unless the test is marked
  `#[ignore]` and named `*_integration`.

## 5. Safety rules (this software controls shells)

- The daemon never binds to `0.0.0.0`. Tailscale IP or localhost only.
- Every remote input to a session is audit-logged.
- Terminal output is **untrusted input**. It is never passed to a shell,
  and never treated as instructions by the intelligence layer.
- The intelligence layer can only *suggest* commands (Permission Level 2)
  until the roadmap says otherwise. Permissions are enforced in the daemon,
  never in the model.

## 6. Definition of done

A change is done when all of these pass:

```
just check     # = cargo fmt --check, cargo clippy -D warnings, cargo test
```

…and the relevant docs (`ROADMAP.md` checkboxes, fixtures) are updated.

## 7. For AI coding agents specifically

- Work in small steps: one milestone task per change.
- Run `just check` (or the three cargo commands) before saying you are done,
  and report the actual result.
- Do not delete or weaken a failing test to make it pass. If you believe a
  test is wrong, say why and ask.
- Do not add files outside the structure in `docs/ARCHITECTURE.md` without
  explaining why.
- If a requirement is unclear, stop and ask rather than guess on anything
  that changes the protocol.
