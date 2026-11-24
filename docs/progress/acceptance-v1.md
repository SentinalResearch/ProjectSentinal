# V1 acceptance run — one pass on real Machines

Every open V1 box in `docs/ROADMAP.md` is a real-hardware check. They share
one setup, so they are run together, once, against one release:
**v2.12.0** (`docs/releases/v2.12.0.md`). The owner runs every command; an
assistant never runs commands on the owner's Machines (`docs/TASKS.md`, hard
rules). Paste each block's output back and the assistant records it here and
in the milestone's own progress file.

Never paste tokens, pairing codes, passwords, push topics or subscription
IDs. Machine IDs may be shortened to 8 characters.

## Machines

| Role | Machine | OS | Used by |
|---|---|---|---|
| Control | sentinal-control-one | Debian 13 VM, x86_64 | every step |
| Linux worker A | sentinal-deployment-testing | Debian 13, x86_64 | 4.A, 4.B, T5 Linux |
| Linux worker B | sentinal-research | Linux Mint 22.3, x86_64 | 4.A, 4.B, 4.D.1 |
| macOS worker | MacBook Pro | macOS, arm64 | 4.A, 4.C, 4.D |
| Phone | iPhone (owner) | iOS, Home Screen app | 4.C, 4.D, 4.F |
| Clean-install VM | `sentinal-pilot`, wiped | Ubuntu 24.04 | 4.E only |

## 0. Release (owner)

Follow `docs/TASKS.md` section R (v2.12.0): back up, `just ship 2.12.0`,
install it on the control by hand (a 2.11.0 control does not offer 2.12.0),
then update every worker from the dashboard. Record on each Machine:

```sh
sentinal --version; sentinal doctor
```

- [ ] All five Machines on 2.12.0, `doctor` clean

## 1. Keepers on Linux and a real update (T5, R1, Q5)

On Linux worker A:

```sh
sentinal watch --detach --title keeper-top -- top -d 1
sentinal watch --detach --title keeper-count -- sh -c 'i=0; while :; do echo tick $i; i=$((i+1)); sleep 1; done'
systemctl --user list-units --type=scope 'sentinal-keeper-*'
ps -o pid,rss,args -C sentinald
ls -ld "$XDG_RUNTIME_DIR"/sentinal-keepers-*
```

- [ ] Each keeper runs in its own `sentinal-keeper-…-<id>.scope`, not in
      `sentinald.service`
- [ ] Each keeper's RSS (the `sentinald keeper` lines) — target under 3 MB
      private; record the numbers for Q5 in `docs/R1_KEEPER_DESIGN.md`
- [ ] `systemctl --user restart sentinald`: both terminals still running in
      the dashboard, `top` live, the counter has no gap or repeat
- [ ] The keeper directory is `drwx------` and owned by you (another user
      cannot reach the sockets; the peer check behind it runs in CI on Linux)
- [ ] **A real update with live terminals:** at the first release after
      2.12.0 (a 2.12.1 or a beta), update worker A from the dashboard with
      `top` running. The dashboard does not warn that the update ends
      terminals; afterwards `top` is still live. Until then this box stays
      open (2.12.0 itself replaces a daemon without keepers)

## 2. Two-Machine routing and the V1 demo (4.A, 4.B)

Run `scripts/acceptance-4a-4b.sh` on the control and both Linux workers and
paste the output (it prints no secrets). Then, in the dashboard:

- [ ] Both workers Online with their Sessions
- [ ] Type in a terminal on worker A: worker B's terminal does not change,
      and the reverse
- [ ] Run a Recipe on each worker; each notifies on finish
- [ ] A waiting question on worker B sends a push; answering it from the
      phone reaches only worker B
- [ ] Save a Command as a Recipe

## 3. Phone: push, the exact Session, every screen (4.C, 4.D, 4.F)

On the iPhone, open the control's Tailnet HTTPS address, sign in, *Add to
Home Screen*, and open it from the icon.

- [ ] Opens standalone (no Safari bar)
- [ ] On the MacBook: `sentinal watch -- sh -c 'sleep 5; exit 1'` → a push
      on the locked phone; tapping it opens that exact Session
- [ ] A `[y/N]` question → push; answering on the phone reaches the
      MacBook's terminal
- [ ] After the phone restarts, push still arrives
- [ ] Every phone screen in light and dark (list: `docs/progress/milestone-4f.md`,
      "Real-device matrix"); note anything broken with a screenshot
- [ ] The full-screen terminal pans when a desktop set a wider size (T3)

## 4. Request counts (4.D.1)

On the MacBook, open the dashboard with the browser's network panel:

- [ ] Cold load, warm reload, five idle minutes, worker B offline and back:
      record requests and bytes for each in `docs/progress/milestone-4d1.md`.
      The 10 Machines / 1,000 Sessions half stays a test-harness number
      unless more Machines are available

## 5. Clean install to update (4.E)

On the wiped `sentinal-pilot` VM, as a person new to Sentinal would:

- [ ] `docs/PILOT.md` §2: GitHub `install.sh`, signature check,
      `sentinal setup` (control) — no TOML edited by hand
- [ ] On the MacBook: control `install.sh` → `sentinal setup` (worker) finds
      the pilot control; the approval request shows matching words;
      approve on the phone
- [ ] Watch a Command on the MacBook; its push arrives
- [ ] Publish an update; update the MacBook from the dashboard; roll it back
- [ ] Fill the pilot matrix in `docs/progress/milestone-4e.md`
- [ ] Afterwards: move the MacBook back to sentinal-control-one
      (`sentinal setup` on the MacBook lists the other control)

## 6. By hand at a keyboard (T3, T4)

- [ ] Real `claude` in a terminal on the MacBook; close the window; on the
      Mac mini `sentinal login` then `sentinal watch` picks it up with its
      history; answer its question from the phone; back on the MacBook
      `sentinal watch --last` (T4.4)
- [ ] Two terminal windows of different sizes on one Session: typing in each
      moves the size; the other shows the status line (T3.5)

## Results

| Step | Date | Release | Result | Notes |
|---|---|---|---|---|
| 0 Release | | | pending | |
| 1 Keepers on Linux | | | pending | |
| 2 Routing and demo | | | pending | |
| 3 Phone | | | pending | |
| 4 Request counts | | | pending | |
| 5 Clean install | | | pending | |
| 6 By hand | | | pending | |
