# uh-cli

The `unharnessed` binary and composition root.

## Scope

- Argument parsing, mode selection and exit codes. Modes: `-p` (print, `text`
  or `json` via `--mode`), `--mode acp`, and the terminal UI when launched
  in a terminal without either. Public commands: `help`,
  `mcp add|list|get|remove`, `model list`, `session export`, `prompt`. The
  old TUI, the daemon modes and their commands (`agents`, `attach`, `status`, `doctor`,
  `shutdown`, `update`, `schedule`, ...) were removed; the old names print
  removal guidance.
- **Print runtime** (`print_runtime.rs`): drives the in-process session engine
  for `-p`. It admits the prompts, runs the turn-boundary compaction checks
  (`print_boundary.rs`: provider retry/failover, overflow compact-and-retry,
  threshold), emits the json event stream (session header,
  `message_update` deltas, tool events, `compaction_start`/`compaction_end`,
  `goal_update`, `session_action_update`), and picks the terminal result
  (stdout, stderr, exit code). Once the prompts settle, it runs the session's
  wakes (a background `bash()` completion, a goal backoff, a quota reset) as
  further turns, waiting at most `--background-wait-ms` (default 10 minutes)
  each time; a goal waiting on a running command outlasts that limit. The kernel shuts down before the process exits: a persisted
  session flushes its snapshot, so `-c`/`-r` revive the Python namespace, and
  the background commands still running die with it.
- **Goals and autonomous mode** (`print_goal.rs`, `print_autonomous.rs`): the
  `--goal` seed rides the first turn; the in-loop continuation hook runs an
  active goal's or the autonomous run's natural continuations inside one agent
  run, with the budget-limit steer and the threshold-held continuation drained
  as follow-up runs. A goal that ends `blocked` ends the run with exit code 0.
- **Session slash commands** (`print_session_command.rs`): prompts that are
  `/compact`, `/goal` or `/autonomous` run through the uh-core session-command
  executor instead of reaching the model.
- **ACP** (`--mode acp`): wires uh-daemon's ACP transport to the same headless
  engine builder, and hands it the provider logins.
- **Provider logins** (`provider_login.rs`, `subscription_login.rs`,
  `codex_subscription_login.rs`, `prime_inference_login.rs`): the provider
  list, the API-key store and the browser sign-ins, implementing uh-daemon's
  `provider_auth` contract over the auth-panel channel. The ACP
  `_unharnessed/providers/*` methods drive them (the VS Code extension's
  login screens).
- **MCP login logic** kept for a future front-end (`mcp_login.rs`,
  `client_auth.rs`): the MCP auth flows, driven through the same channel.
- **Terminal UI**: a terminal launch without `-p`/`--mode` opens uh-tui's
  live session. It starts this binary again as its agent (`--mode acp`),
  with the launch's own options (`args::agent_args`: the arguments without
  the messages and the `@file`s, which the UI sends itself) from the folder
  the launch was in, and is its ACP client. The UI can leave that
  conversation for another (a new one, a saved one), on an agent started
  for it: it gets the same arguments without what chose the launch's
  (`--continue`, `--resume`, `--fork`; `--no-session` too for a saved
  one). `--resume` without a session starts on a new conversation, the UI
  open on the saved ones. `UNHARNESSED_TUI_RECORD=<file>` records
  the frames exchanged. The UI opens a server's port in the browser with
  the command `BROWSER` names (its words, then the address), else as the
  sign-ins do (`uh_core::platform::browser`). The hidden `--tui-replay <recording>` flag hands
  its arguments to uh-tui (a recorded conversation on the screen, or drawn
  into text with `--at`).
- **Packaged layout**: the binary plus `package.json`, `unharnessed-runtime/`
  and `skills/`, resolved from the executable's directory (a link to the
  binary is followed to it: `uh_types::platform::executable_dir`); the hidden
  `--unharnessed-bootstrap` flag pre-builds the kernel venv. Verifier:
  `tests/packaged_layout_e2e.rs`. The terminal install is this layout:
  `scripts/unharnessed-package.sh` archives it, `install.sh` at the
  repository's root puts it in `~/.local/share/unharnessed/<version>/` behind
  the link `~/.local/bin/unharnessed`. Verifier:
  `scripts/unharnessed-install-e2e.py`.

## Non-goals

No business logic: it composes uh-core, uh-daemon, uh-ai and uh-agent through
their public APIs.

## Public API

The `unharnessed` executable surface (flags, subcommands, exit codes);
`print_runtime` is public for the integration verifiers. Everything else is
`pub(crate)`.

## Depends on

uh-types, uh-ai, uh-agent, uh-core, uh-daemon, uh-tui (one-way, composition root).
Verifiers: `tests/print_runtime_e2e.rs`, `tests/acp_mode_e2e.rs`,
`tests/acp_config_option_e2e.rs`, `tests/continue_guard_e2e.rs`.
