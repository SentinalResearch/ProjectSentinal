# Plan: search, autocomplete, and an AI-friendly Sentinal

Status: **proposal for review, 2026-11-01.** Nothing here is built. It
replaces nothing in `ROADMAP.md` until approved; Milestones 5–7 and their
prompts (`prompts/06–08`) are folded in and renumbered below.

## 1. What this plan does

Sentinal today is built for a person watching terminals. This plan adds
three things, in this order:

1. **Search and autocomplete that feel instant**: one search across
   everything Sentinal knows, and command completion in the shell, the
   prompt, and the phone keyboard. No model involved.
2. **AI agents as first-class users**: an agent (Claude Code, Codex, a
   script, Sentinal's own assistant) can list, watch, wait on, and act on
   Sessions through the same API as the UI, with its own scoped token,
   permission level, and audit trail. It loads one tool catalogue (MCP or
   JSON) instead of scraping screens.
3. **AI reading terminals**: the screen translator already prototyped in
   `eval/` (fine-tuned Qwen3-0.6B: F1 0.978 synthetic test, 0.914 on real
   logs, 0 secret leaks) runs beside the deterministic watcher and turns
   what it reads into the same structured Events: progress, errors,
   prompts. Those feed the dashboard, the island ticker, and notifications.

It also covers the reverse case: **AI agents running inside Sentinal
Sessions** (Claude Code in a terminal), so Sentinal knows when one is
waiting for your approval and can say so on your phone.

## 2. Where we are

| Area | Today |
|---|---|
| Command search | SQLite FTS5 over Command text (`store/records.rs`); History page and `find` in the prompt |
| Prompt search | Client-side fuzzy match over what the browser already holds (`promptGrammar.ts`) |
| Autocomplete | None. Milestone 5 specifies frecency ranking and a shell widget |
| Agent access | The browser WebSocket API and CLI. No agent tokens, levels, or tool catalogue (Milestone 6 §1 specifies them) |
| Terminal understanding | Deterministic adapters (`crates/watcher`): pytest, cargo, training, generic. Translator: offline eval (`docs/progress/v2-translator-eval.md`), FunctionGemma training and eval format (`eval/functiongemma.py`), and the guarded translator as a command-line service (`intelligence/`) |
| Notifications | Rules over stored structured Events only; never raw output (`SECURITY.md`) |

## 3. Rules that do not change

These come from `AGENTS.md` and `PLAN.md` and bind every milestone below.

1. **Structured state first.** Every feature works with no model running.
   A model can add Events; it can never be the only way something works.
2. **Terminal output is untrusted.** It is data to a model, never
   instructions. It is never executed, and it never becomes a URL.
3. **Permissions are enforced in the daemon**, by token, never by asking a
   model or a client to behave.
4. **Nothing destructive in one step.** Input, ^C, kill, and running a
   recipe need a person's confirmation until a later milestone says
   otherwise, and even then only with an approval card.
5. **Every agent action is attributed and audited**: which agent, which
   token, which level, and what it asked.

## 4. Permission levels

Agents get their own tokens (`sentinal agent token create --name claude --level 2`),
separate from the Machine token and browser Sessions, revocable one by one.

| Level | Name | May | Example tools |
|---|---|---|---|
| 0 | Observe | Read structured state, search, watch Events | `list_sessions`, `get_session`, `search`, `watch` |
| 1 | Annotate | Level 0, plus write notes, summaries, and *proposed* Events (marked as inferred) | `propose_events`, `add_note`, `summarise` |
| 2 | Suggest | Level 1, plus suggestions a person must accept | `suggest_command`, `suggest_answer`, `notify` |
| 3 | Act with approval | Request an action; it runs only after a person approves it on desktop or phone | `request_input`, `request_answer`, `request_recipe_run`, `request_interrupt` |
| — | Act alone | Not offered. Out of scope for this plan | — |

Level 3 exists only after Milestone 7.0 is approved separately.

## 5. Milestones

Each milestone is one PR series, test-first, and ends with `just check`
and a demo. They follow the roadmap's format and can be pasted into
`ROADMAP.md` once approved.

### Milestone 5.S — Search everywhere (no model)

Goal: one search, fast and ranked on the daemon, used by the prompt, History, and the phone.

- [ ] `SEARCH { query, scopes, limit, cursor }` on the control: Sessions,
      Commands, Events, recipes, Machines, projects, and Settings pages, as
      one ranked list with `kind`, `title`, `detail`, `target`, and `score`
- [ ] FTS5 indexes for Events and Session titles beside the Commands index;
      the control's read model holds every Machine's results
- [ ] Ranking: prefix > word start > substring > fuzzy; recency and "needs
      you" boosts; same formula on desktop and phone (table-driven tests)
- [ ] The prompt calls `SEARCH` for bare text and `find`, so results include
      Machines that are not loaded in the browser yet
- [ ] Filters as prompt grammar: `find failed pytest on rtx-pc since yesterday`
- [ ] Output search, **opt-in per Session** (FULL capture only): search the
      saved log with secret-pattern redaction in results; never indexed by default
- [ ] Budget: p95 < 50 ms on 100 000 Commands and 500 000 Events (timed `#[ignore]` test)

**Demo:** type `oom` in the prompt and see the Session, the Event, and the
Command where CUDA ran out of memory, across two Machines, in under 50 ms.

### Milestone 5.A — Autocomplete (no model)

Goal: the command you want, in fewer keystrokes, wherever you type. Folds in `prompts/06` §3.

- [ ] `COMPLETE { session_id?, prefix, cwd, project? }` on the daemon and the
      local socket; frecency ranking (count × 7-day half-life) with project,
      cwd, and success bonuses and a failure penalty (weights in config)
- [ ] Shell: zsh widget (Ctrl-Space) and bash/fish equivalents through
      `sentinal hook`; never replaces the shell's own Tab completion
- [ ] Prompt: `run`, `new`, and `find` complete Commands and recipe
      parameters inline (ghost text, Tab to accept)
- [ ] Phone keyboard: the chip strip shows the top completions for what is
      on the current line, replacing the static "recent commands" chips
- [ ] Projects (`prompts/06` §1): auto-assign by cwd, so completion ranks
      the current project first
- [ ] Budget: p95 < 20 ms on 100 000 history rows; the spec example ranks
      `python train.py` (31×) before `python train.py --config…` before the recipe

**Demo:** in a fresh shell, `py` then Ctrl-Space offers `python train.py
--config moe.yaml` first because it is this project's most frequent
successful command; the same appears as the first chip on the phone.

### Milestone 6.0 — Agent API: Sentinal as a tool for AI

Goal: any AI agent can load Sentinal's functions and watch or work with Sessions safely. Folds in `prompts/07` §1.

- [ ] Agent tokens with levels 0–3 (§4), created, listed, and revoked from
      Settings → Agents and `sentinal agent token`; enforced in the daemon;
      over-level requests rejected and audited
- [ ] One tool catalogue generated from Rust types: `docs/agent-tools.json`
      (JSON Schema), with a test that it is current
- [ ] **`sentinal mcp`**: an MCP server over stdio (and HTTP on the control)
      exposing the catalogue, so Claude Code, Claude Desktop, or any MCP
      client loads Sentinal with one line of config
- [ ] Watching built for agents: `watch { sessions?, kinds?, since_cursor }`
      streams Events, and `wait_for { session, until: state | event_kind,
      timeout }` blocks until something happens, so agents never poll screens
- [ ] `--json` on every CLI read command, with the same shapes as the API
- [ ] Attribution: the audit log and the UI show "via claude (level 2)" on
      anything an agent touched; a per-agent activity page in Settings
- [ ] `docs/AGENT_API.md` plus an `llms.txt` at the control's root that
      describe the tools in plain language for model context

Initial catalogue (level in brackets):

| Tool | What it does |
|---|---|
| `list_machines` [0] | Machines with status, hardware, load |
| `list_sessions` [0] | Sessions with state, capture, progress, needs-you flag |
| `get_session` [0] | One Session: Command, health, last 20 Events, metrics, prompt |
| `search` [0] | 5.S search |
| `complete` [0] | 5.A completion |
| `watch` / `wait_for` [0] | Event stream / block until a state or Event |
| `get_output_tail` [0, opt-in] | Last N lines of a FULL Session as **untrusted data**, redacted, only if the owner enabled it for that Session or agent |
| `propose_events` [1] | Add inferred Events (see 6.2) |
| `add_note` / `summarise` [1] | Journal notes on a Session or project |
| `suggest_command` / `suggest_answer` [2] | A card with "Open in terminal" or y / n; never typed by itself |
| `notify` [2] | A notification through the owner's rules and rate limits |
| `request_*` [3] | Milestone 7.0 only |

**Demo:** in Claude Code, add Sentinal as an MCP server, ask "start the
eval on pop-os and tell me when it finishes"; Claude finds the recipe,
proposes the run (you approve it in Sentinal), then calls `wait_for` and
reports the result. The audit shows every step "via claude".

### Milestone 6.1 — Agent-aware Sessions: AI running inside Sentinal

Goal: when an AI coding agent runs in a Sentinal terminal, Sentinal knows what it is doing and when it needs you.

- [ ] Deterministic watcher adapters for agent CLIs (Claude Code, Codex
      CLI, aider, Gemini CLI), chosen from the Command like other adapters
- [ ] Their approval prompts ("Allow this edit?", "Run this command?")
      become WAITING with the offered answers, so the dashboard card, the
      island, and a push notification show it, and y / n works from the phone
- [ ] Activity (tool calls, files edited, tests run) becomes Events; "agent
      finished" and "agent errored" become notifications under the usual rules
- [ ] Cost and token lines, where printed, become metrics
- [ ] A Session badge "AI agent" and a filter on the dashboard

**Demo:** Claude Code runs a refactor on pop-os; you get a push "Claude
Code wants to run `cargo test`, allow?", answer from the lock screen, and
see it finish in "While you were away".

### Milestone 6.2 — Screen translator in the product: AI reads terminals

Goal: progress, errors, and prompts for tools no adapter knows, turned into the same structured Events and notifications. Builds on `eval/` and `prompts/07` §2–4.

- [ ] `intelligence/` service (Python), a separate optional process
      using an agent token at **level 1**: it reads screens and can only propose Events.
      *Started:* sanitising, guards, pacing and a `translate` command line
      exist and are tested; the agent token and daemon connection wait on 6.0
- [ ] Runs a local OpenAI-compatible endpoint (the fine-tuned
      `sentinal-qwen3-0.6b` on llama.cpp or vLLM on the owner's GPU); no
      cloud model by default
- [ ] Triggers: only for FULL Sessions with no confident deterministic
      adapter, on new output, debounced (at most once every 10 s per
      Session), on the last screen only
- [ ] Guards in code, not prompt: constrained decoding to the tool schema;
      every number must appear in the screen text (grounding); no URLs,
      paths outside the cwd, or secret-shaped strings; ANSI stripped; the
      screen wrapped as untrusted data; prompt-injection test set must
      produce 0 non-schema outputs
- [ ] `propose_events` in the daemon re-checks the grounding, marks Events
      `source = INFERRED` with confidence, and keeps deterministic Events
      authoritative when both exist
- [ ] UI: inferred progress and Events show a small "inferred" marker;
      notifications from inferred Events need confidence ≥ 0.9 and a rule
      that allows inferred sources (off by default)
- [ ] Promotion gate before it is on by default: a hand-labelled set of
      ≥ 200 real screens (`ROADMAP` V2 item) with F1 ≥ 0.95, precision
      ≥ 0.97 on errors and prompts, 0 secret leaks, p95 < 1 s on the target GPU

**Demo:** a training script with a progress format no adapter knows shows
"epoch 12/40 · loss 0.81" on the dashboard and the island (marked
inferred), and a CUDA error in it sends one push notification.

### Milestone 6.3 — Assistant features (levels 1–2)

Goal: answers and summaries grounded in structured state. Folds in `prompts/08` §2.

- [ ] Summaries and "While you were away" written from structured state by
      templates; a model only rephrases, and the template is always the fallback
- [ ] `ask` in the prompt (`? why did pytest fail on rtx-pc`): a read-only
      answer that cites the Events and Commands it used, each a link
- [ ] Suggestions, rules first: CUDA out-of-memory → compare with the last
      successful run and suggest the differing argument (e.g. batch size)
      as a card with "Open in terminal"
- [ ] Triage heartbeat: a deterministic pre-filter; a model only for
      "should this notify?", with precision ≥ 0.9 on a frozen set

### Milestone 7.0 — Act with approval (level 3, separate approval)

Goal: an agent may *request* actions; a person approves each on desktop or phone.

- [ ] `request_answer`, `request_input`, `request_recipe_run`,
      `request_interrupt`: each creates an approval card (what, where,
      why, which agent) on the dashboard and as a push
- [ ] Approve with the same hold-to-confirm as ^C; approvals expire; every
      decision is audited
- [ ] Per-agent policy: which tools, which Machines and projects, daily
      limits; a kill switch that revokes every agent token at once

## 5.1 Models

FunctionGemma 270M (`google/functiongemma-270m-it`) is the default small
model. It is built for function calling, and at 270M it runs on a Mac or a
CPU as well as the GPU machine, so every Machine owner can run it locally.

| Job | Milestone | Model | Status |
|---|---|---|---|
| Screen → Events (translator) | 6.2 | FunctionGemma 270M fine-tuned, head-to-head with `sentinal-qwen3-0.6b` (F1 0.978 test); whichever passes the gate ships, FunctionGemma preferred on a tie | Format, training and scoring done (`just train-functiongemma`); first run on pop-os next |
| Natural language → one tool call (`route`): the prompt's fallback when the grammar does not match, e.g. "restart the eval on pop-os" → `suggest_command` | 6.3 | FunctionGemma 270M fine-tuned on the 6.0 catalogue | Needs the 6.0 catalogue; dataset of ≥ 50 hand-written seeds plus a hand-checked frozen test set |
| Triage ("should this notify?") | 6.3 | Rules first; a model only if rules miss, precision ≥ 0.9 | Later |

Each model is promoted only through its gate on a frozen test split; the
deterministic path stays the fallback when no model is running.

## 6. Risks and how the plan handles them

| Risk | Handling |
|---|---|
| Prompt injection through terminal output | Output is data, never instructions; schema-only outputs; grounding checks in the daemon; injection test set as a release gate |
| Secrets in output reaching a model or an agent | Output tail is opt-in per Session or agent, redacted; the translator runs locally; nothing leaves the Tailnet by default |
| Notification spam from inferred Events | Off by default; confidence threshold; the same rate limits and rules as today |
| An agent doing something destructive | No level above 3; level 3 only requests; every request needs a person; tokens revocable per agent |
| Search or completion becoming slow | Budgets tested in CI (`#[ignore]` timed tests on generated data) |
| Model quality drifting | Frozen test sets; promotion gates; deterministic adapters stay authoritative |

## 7. Suggested order

`5.S → 5.A → 6.0 → 6.1 → 6.2 → 6.3`, then `7.0` after its own review.
5.S and 5.A help everyone right away; 6.0 is the foundation every AI
feature uses; 6.1 is the quickest visible AI win (it is deterministic);
6.2 needs the real-screen labelling done first.

## 8. Decisions (owner, 2026-11-01: "go with the recommendations")

1. **Agent levels:** stop at level 2 (suggest). Level 3 is not offered; the
   protocol cannot even name it. Milestone 7.0 needs its own review first.
2. **MCP first:** `sentinal mcp` over stdio first; HTTP/JSON follows from the
   same catalogue.
3. **Output access for agents:** `get_output_tail` is off by default, enabled
   per agent and per Session, redacted.
4. **Where models run:** local only. No cloud model.
5. **Labelling:** 6.1 (agent CLIs, deterministic) comes before 6.2.
6. **Output search (5.S):** opt-in per Session only.
