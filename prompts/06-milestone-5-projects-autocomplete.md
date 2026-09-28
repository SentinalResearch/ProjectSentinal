# MILESTONE 5 — Projects, command library, autocomplete (V2 part 1)

## 1. Projects

- `Project { id, name, root_dir, repo_url: Option, git_branch: Option }` in
  protocol (fixture + TS).
- Auto-assign a session to a project when its cwd is inside `root_dir`
  (longest match wins). Projects defined in `projects.toml` or created from
  the UI.
- Git info read by running `git` with a 1 s timeout, never blocking the PTY path.
- UI: project page grouping sessions, commands and events across machines.

## 2. Command library

Unified search over built-in recipes, saved recipes and history, grouped by
category, with "used N×", last exit code and last run time.

## 3. Autocomplete

- Endpoint `COMPLETE { session_id, prefix, cursor }` → ranked list.
- Candidates: history (same project first, then same machine, then all),
  recipes, successful commands only by default.
- Score = frecency (count × recency decay, half-life 7 days)
  + project match bonus + cwd match bonus + success bonus − failure penalty.
  Weights in config; document the formula.
- Latency budget: p95 < 20 ms on 100 000 history rows. Benchmark it
  (`cargo bench` or a timed test with a generated DB, `#[ignore]`).
- Shell side: zsh widget bound to a key (e.g. `Ctrl-Space`) calls `ta complete`
  and shows results; bash/fish equivalents where practical. Never replaces
  the shell's own Tab completion.
- No model involved in this milestone.

## Tests first

- Project matching: nested roots, no match, symlinked paths.
- Ranking: table-driven tests with fixed history and fixed "now"; the
  spec example (`python tr` → `python train.py` 31×, then with `--config`,
  then the recipe) must come out in that order.
- Failed commands are excluded by default.

## Done when

`just check` passes, benchmark meets budget, report sent. Stop.
