# Terminal upgrade: progress

Prompt: `prompts/13-terminal-upgrade-package.md`. Tasks: `docs/TASKS.md`
section T. Point-in-time evidence, as the other reports in this directory.

## T2: authoritative screen on libghostty-vt (2026-11-22)

Tested on a working tree based on commit `13781d8`, macOS 27.0 (Apple
silicon, Mac mini), rustc 1.97.1, Zig 0.17.1 (Homebrew).

### Build gate

The gate ran in a throwaway crate outside the repository, with the crate the
daemon now uses: `ghostty-vt` `=0.9.0` (libghostty-vt release 1.0.1).

| Target | Result |
|---|---|
| aarch64-apple-darwin | Builds and runs; links the release's prebuilt XCFramework (sha256-pinned), no Zig |
| x86_64-apple-darwin | Builds and links; not run (no Rosetta on the test machine). Intel Macs were dropped afterwards (owner, 2026-11-22) |
| x86_64-unknown-linux-musl | Builds a static binary from the pinned source with Zig 0.17.1; not run (no Linux machine or working Docker) |
| aarch64-unknown-linux-musl | Same |

Offline: the Linux source build reads the source and dependency tarballs
named in the crate's `GHOSTTY.lock`; `zig build --system` then fetches
nothing. `scripts/fetch-terminal-engine.sh` fetches those files (and Zig on
Linux) before the release build, pinned by sha256, and the build runs with
`--offline`. Cross-building Linux from a Mac needs Zig as the linker with
`-C link-self-contained=no`; the release workflow builds Linux on Linux.

| Measure | vt100 0.16 | libghostty-vt 1.0.1 |
|---|---|---|
| 100 MB mixed output, 8 KiB chunks, one thread | 0.42 s (237 MB/s) | 0.18 s (562 MB/s) |
| Stripped binary, macOS arm64 (minimal program) | 393 KB | 1.6 MB |
| Stripped static binary, Linux x86_64 | 433 KB | 2.8 MB |
| Stripped static binary, Linux aarch64 | 408 KB | 2.4 MB |
| Start and exit, mean of 200 runs | 2.72 ms | 2.83 ms |
| Create a 120×40 terminal | n/a | 7.5 µs |

Parity: 119 streams (the 9 recorded logs in `crates/watcher/tests/logs`, 96
escape-bearing strings from the watcher and daemon tests, 14 hand-made
streams) through both engines at 80×24. 117 identical. The two differences:

1. Tab stops set with `ESC H` after `ESC[3g`: vt100 puts the next character
   one column early; libghostty-vt follows the stops.
2. DEC line drawing (`ESC(0`): vt100 shows `lqqk`; libghostty-vt draws `┌──┐`.

Both are vt100 bugs. The owner chose libghostty-vt.

### What was built

| Part | Where |
|---|---|
| The engine, the only code that names it: feed (query answers and other effects dropped), resize with reflow, snapshot, rows, alternate flag, cursor, text lines | `crates/daemon/src/screen/engine.rs` |
| One screen thread per Session; bounded feed channel (64 reads); draws, then publishes; snapshots answered between feeds with a receiver subscribed at that moment; fail open without an engine | `crates/daemon/src/screen/mod.rs` |
| Snapshot on the alternate screen: the main screen first (from a copy of the engine that leaves the alternate screen), then the alternate screen, so a viewer has the main screen back when the program leaves | `engine.rs`, `snapshot` |
| History in the snapshot: about `attach_history_lines` (`daemon.toml`, default 2,000); a second copy asks for more when libghostty's page-sized trimming left under 90 % | `engine.rs`, `config.rs`, `entry.rs` |
| Reader, watcher rows, attach, lag redraw, resize and `scrollback` moved to the screen; `vt100`, `screen_history.rs` and the line ring on each Session are gone | `session_manager/{tasks,operations,lifecycle,external}.rs`, `api/attach.rs` |
| Release workflow fetches the engine's files first | `.github/workflows/release.yml`, `scripts/fetch-terminal-engine.sh` |

`SequenceBoundary` (T1.5) stays: libghostty-vt cannot carry an unfinished
UTF-8 character into a snapshot.

### Tests

New in `crates/daemon/src/screen/tests.rs`: output drawn then published in
order; a snapshot and its receiver split the output exactly; leaving the
alternate screen after an attach brings back the main screen (failed before:
the T1.5 finding); a viewer on the alternate screen sees the program; history
snapshot carries about the configured lines and a plain one none; colours and
cursor survive; resize reflows; watcher rows and flag; queries neither
answered nor piling up; fail open without an engine; a viewer attaching at
every byte of a recorded stream ends with the daemon's screen; three recorded
logs draw as the program meant (`\r` progress overwritten in place). Moved off
vt100: the ScreenSource, lag-redraw and attach integration tests. Config test
for `attach_history_lines`.

`just check`: passed. Rust 1,438 passed, 0 failed, 22 ignored; web 586
passed (107 files); script suites passed.

`just test-e2e`: 20 passed (2.4 min).

### Tried by hand (`just dev` QA instance, web on 127.0.0.1:5283)

A Session printed 40 coloured lines, opened a full-screen view, and closed it
75 s later. The web terminal opened during the full-screen view showed the
monitor with its reverse-video title. When the program left, that viewer
showed the 40 lines and "back on the main screen" (before T2 it would have
shown only the last line). A reload showed the history, scrollable to the
start. The bytes the daemon sent were checked over the WebSocket: each
history line resets its style. The reloaded history looked slightly heavier
than live output in the browser; the bytes carry no bold, so this is
how the browser draws it, not the daemon's output.

### Not verified

- Linux binaries were built, not run.
- `scripts/fetch-terminal-engine.sh` was checked with shellcheck, not run
  (it downloads release files); the workflow change has not run in CI.
- Linking the Zig-built library with the musl toolchain CI uses
  (`musl-tools`) was not tried; locally Zig was the linker.
- The CLI still attaches without history; that is T4.
