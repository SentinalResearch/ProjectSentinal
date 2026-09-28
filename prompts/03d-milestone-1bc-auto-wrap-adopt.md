# MILESTONE 1b.C — Auto-wrap and read-only Adopt

Goal: optionally place every eligible new interactive terminal into FULL
capture, and observe an already-running same-user process without taking
control of it.

Depends on completed 1b.A Watch and 1b.B external hook. Adopted log parsing is
not in this stage; explicit/detected log metadata may be recorded, but actual
tailing lands in 1b.D after Milestone 3.

## 1. Auto-wrap

Implement:

```console
sentinal auto on
sentinal auto off
sentinal auto status
```

- Off by default. `status` reports the exact config file/block and whether it
  is active for the current shell.
- Reuse the safe marker/atomic-edit machinery from hook install. Do not create
  a second competing rc editor.
- Eligible new interactive shells enter an `AUTO_WRAP` / `FULL` Session using
  the Watch path.
- Skip when any of these applies: `SENTINAL_DISABLE=1`,
  `SENTINAL_SESSION_ID` is set, shell is non-interactive, non-interactive SSH,
  scp/rsync transport, configured IDE task-runner environment, unsupported
  shell, or recursion guard is active.
- Put skip rules in one table-driven pure function. Document exact environment
  signals; do not use a broad `TERM_PROGRAM` rule that disables ordinary IDE
  terminals unnecessarily.
- If the daemon cannot answer, launch the original shell normally within
  300 ms. The fallback is silent and preserves argv, environment, cwd, and
  exit behavior.
- Prevent fork bombs and recursive wrapper invocation with an explicit guard.

## 2. Adopt CLI and process inspection

Implement:

```console
sentinal adopt PID [--log PATH] [-t TITLE]
sentinal adopt --pick
```

- Resolve the target before mutation: PID must be non-zero, currently alive,
  owned by the daemon user, and not already represented by an adopted Session.
- `--pick` lists only same-user candidates and performs no adoption until the
  user selects one. Keep selection logic testable without a real process list.
- Create an `ADOPTED` / `PROCESS` Session rooted at the PID.
- Sample its complete process tree through the existing Metrics path.
- Store title, cwd/executable metadata when safely available; missing metadata
  is not an error.
- On process identity exit, show the approved structured “Ended · exit code
  unknown” result. Never infer `0`, success, failure, or a signal status.
- Adoption is read-only: no signal, ptrace/debug attach, stdin/stdout open for
  writing, reparenting, priority change, or kill path.
- Capability checks reject attach, terminal input, resize, and kill even if a
  remote client sends a handcrafted request.

## 3. Log discovery metadata only

- `--log PATH` validates a readable regular file owned/readable by the current
  user and records it for 1b.D; do not tail it yet.
- Detect redirected stdout/stderr only off the prompt/CLI hot path.
- Linux parser reads injected sample `/proc/<pid>/fd` link data in fast tests.
- macOS parser consumes injected `lsof`-style output; real `lsof` is invoked
  only in ignored/manual integration with a timeout.
- Discovery failure leaves capture at PROCESS and is not fatal.
- If a log candidate exists before 1b.D, UI may say “log detected; tailing not
  enabled yet” but must not claim PROCESS_LOG capture.

## Tests first

- Auto-wrap guard: a table row for every skip and eligible condition,
  including conflicting variables and unsupported shell.
- Auto-wrap deadline: injected connector never answers; fallback selection is
  made by 300 ms with no real sleep over 100 ms. Late daemon success cannot
  create a second shell.
- Fallback preservation: executable, argv, cwd, environment, and exit status
  reach a fake original-shell launcher unchanged.
- Adopt fake process table: same user accepted; other user, PID 0, dead PID,
  PID reuse, duplicate adoption, and self/daemon PID rejected with named
  errors.
- Read-only spy backend asserts no signal/write/debug method exists or is
  called; remote handcrafted kill/input/attach/resize all fail.
- Exit detection produces unknown-exit representation and no finished Command
  with a fabricated code.
- `/proc` and `lsof` sample parsers: stdout, stderr, same file, deleted file,
  pipe/socket/device, permission error, and no candidate.
- Web: PROCESS badge, Metrics, “exit code unknown,” disabled terminal actions,
  and pre-1b.D log-detected hint.
- Integration `auto_wrap_integration` and `adopt_process_integration`
  (`#[ignore]`): real supported shell fallback/entry and a real same-user
  child process on Linux/macOS.

## Done when

Every auto-wrap skip rule and 300 ms fail-open is proved, adoption is visibly
read-only with live Metrics and unknown exit, `just check` passes, ignored
integrations run on the available OS, docs/Roadmap 1b.C are updated, and the
report lists platform-specific discovery gaps. Commit and stop before
Milestone 3.
