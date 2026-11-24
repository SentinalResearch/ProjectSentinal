# `sentinal watch`: start anywhere, pick up anywhere (T4 proposal)

Status: **approved 2026-11-22** with the four recommended decisions, and
**built** (T4.1–T4.3; `docs/TASKS.md` section T). The control needed no
change: its existing owner sign-in (`/api/v1/auth/login`) is what
`sentinal login` uses. Source: `prompts/13-terminal-upgrade-package.md` §T4 and owner decision
3 there ("`sentinal watch` as the one pick-up command"). This refines the
prompt's sketch against the code after T3 (`b27e643`).

## The idea in one paragraph

You start `claude` in a terminal at your desk, close the laptop, answer a
question from your phone, and later open any terminal on any of your Machines
and type `sentinal watch`. It shows what is running everywhere, the Sessions
waiting for you first, and puts you back in the one you pick, with what
happened while you were away in your scrollback. One command to start work
and one to come back to it.

## What the code does today (read 2026-11-22)

- `sentinal watch` with no Command always creates a new shell Session and
  attaches; inside an already-watched terminal it does nothing (exit 0).
  `sentinal attach ID` attaches to a local Session by number.
- The CLI talks only to this Machine's daemon, with this Machine's token.
- The control routes a terminal to another Machine only for a signed-in
  owner browser (`control/proxy.rs`, `owner_target`): it refuses every other
  credential, this Machine's token included. **So a CLI has no way to reach
  a Session on another Machine today.** A browser sign-in lasts 24 hours.
- Attaching from the CLI asks for no history (`ATTACH.history` false), so a
  picked-up Session shows only its current screen.
- Nothing in the repository runs a bare `sentinal watch` from a script; the
  scripts and guides use `-- COMMAND` or `--detach`, which keep working
  unchanged. `sentinal auto` does not go through `watch`.

## The command

```
sentinal watch -- COMMAND…        start COMMAND in a new Session and attach (unchanged)
sentinal watch                     pick up: list live Sessions, choose one
sentinal watch NAME|ID             pick up that Session
sentinal watch --last              pick up the Session most recently active
sentinal watch --new               start a new shell Session (today's bare `watch`)
sentinal watch --machine NAME …    only that Machine's Sessions
```

`-t`, `--group`, `--exit`, `--detach` and `--sandbox` stay as they are and
apply to new Sessions only; with a pick-up they are refused.
`sentinal attach ID` stays, as another way to say `sentinal watch ID`.

### Bare `sentinal watch` on a terminal

```
$ sentinal watch
  #  MACHINE    SESSION                 STATE     RUNNING
  1  gpu-box    claude · ~/api          WAITING   2h 10m   Allow edit to src/lib.rs?
  2  laptop     layout-tests queue      RUNNING   41m      72%
  3  laptop     npm dev                 IDLE      3d
Pick a Session (1–3), n for a new one, Enter for 1: _
```

- Live Sessions only (not finished ones), on every Machine this CLI can see.
  Waiting for you first, then running, then idle; newest activity first
  within each. At most 20 rows, then "and 7 more: sentinal ls --all".
- **None:** starts a new shell Session, as bare `watch` does today.
- **Exactly one:** attaches to it directly, saying which.
- The choice is read from the terminal (`/dev/tty`), never from a pipe.
- Inside an already-watched terminal, nothing changes: it says so and exits
  0, as today (picking up from inside a Session would nest terminals).

### Without a terminal (a script or an agent)

Never prompts. Prints the same list (or `--json`, the `sentinal ls --json`
shape) and exits **2** unless it was told what to do: a Command, an ID, a
NAME that matches exactly one Session, `--last`, or `--new`.
`--detach` still works only with a Command.

### Matching `NAME`

In order, the first rule with any match wins:

1. `ID` (`12`) or `MACHINE:ID` (`gpu-box:12`): that Session.
2. The Session title, exactly (case-insensitive).
3. The coding agent's name (`claude`, `codex`, `vibe`) or the last folder of
   the Session's directory (`api`), exactly.
4. The start of the title or of any word in it (`layout` and `queue` both
   find "layout-tests queue"). Changed while trying it by hand: people type
   the word they remember, not the first one.

One match attaches. Several print the matches and exit 1 ("3 Sessions match
`claude`: …; use the ID"). None prints the list and exits 1. A finished
Session matched by ID says it finished and points at `sentinal ls --all`.

### `--last`

The live Session with the newest activity (output, input, or a state
change) on any Machine visible, or on `--machine`. None: exit 1.

### What you see when you pick up

The history first (about 2,000 lines, `attach_history_lines` from T2.4),
scrolled into your terminal's own scrollback, then the live screen. A new
Session (`--new`, `-- COMMAND`) asks for no history, as today. The Session
keeps T3's one-size rule: picking up does not take the size from a device
that is in use; typing does.

## Sessions on other Machines

This needs an owner credential the CLI does not have. **Proposed:**

```
sentinal login [CONTROL_URL]   sign in to the control as the owner (password from the terminal)
sentinal logout
```

- `login` reads the owner password from `/dev/tty` with echo off (never
  argv, a pipe, or the environment), as `connect` reads its pairing code.
  `CONTROL_URL` defaults to the control this Machine is linked to.
- The control issues the same kind of credential a browser gets: an owner
  sign-in that expires after 24 hours and that "Sign out everywhere" and a
  password change end. It is stored at `<config_dir>/owner-session` (0600)
  and sent only to that control, over HTTPS (`http` only to a development
  control on loopback).
- With it, `watch` lists every Machine's live Sessions from the control's
  read model and attaches through the control's terminal route, exactly as
  the browser does. The worker still enforces capture and audit; its audit
  record shows the control's address, as for browser input today.
- Without it (or when it expired): local Sessions only, and the list ends
  with "Only this Machine. `sentinal login` to see your other Machines."
  A Machine not linked to any control says just "Only this Machine."

Rejected alternatives:

- **Route with this Machine's link credential.** Then any linked Machine
  could type into every other Machine's terminals: one compromised worker
  would reach them all. The control rightly refuses this today.
- **Agent tokens** (Settings → Agents). They are for agents' read tools;
  giving them terminal input widens what a leaked agent token can do.

## Work (T4.1–T4.4, after approval)

- T4.1 Listing, ordering and matching as pure functions with tests; the
  picker; the no-terminal rules and exit codes; `--new`, `--last`,
  `--machine`; history on pick-up.
- T4.2 `login`/`logout` (control side: issue and revoke the CLI sign-in as
  a browser sign-in); remote listing and attach through the control; tests
  with a control and a worker in process.
- T4.3 `docs/CLI.md`, `docs/AGENT_GUIDE.md`, `sentinal watch --help`.
- T4.4 By hand in `just dev`: start `claude` in one terminal, close it, run
  `sentinal watch` in another, answer a question from the phone layout,
  return to the first terminal.

No wire change is needed for local pick-up. Remote pick-up adds the
`login` exchange on the control's HTTP API; terminal traffic uses the
existing route unchanged.

## Decisions for the owner

1. **Bare `sentinal watch` picks up** (and starts a new shell only when
   nothing is running), with `--new` for today's behaviour. Recommended:
   yes, as the prompt proposes. It changes what a bare `watch` does for
   anyone used to it creating a Session.
2. **`sentinal login` with the owner password** for Sessions on other
   Machines, a 24-hour sign-in like a browser's. Recommended: yes.
   Alternative: build local pick-up now and leave other Machines for later.
3. **Sign-in lifetime:** 24 hours, the same as a browser (recommended), or
   longer for the CLI (for example 7 days), which means a longer-lived
   secret on disk.
4. **Without a terminal and without a target, exit 2**, the code
   `sentinal` already uses for usage errors. Recommended: yes.
