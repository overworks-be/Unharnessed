# uh-types

The single shared vocabulary crate. Nothing else is shared between crates.

## Scope

Wire and domain types + serde only:

- `ai`: AI messages, content blocks, tool calls, usage, stream events.
- `session`: the session JSONL entry schema. Fields written only by earlier
  builds (`rlmDepth`, compaction `harnessDigest`) stay so older session files
  read and rewrite losslessly.
- `goal`: the thread-goal wire state (`GoalState`/`GoalStatus`) shared by the
  goal engine (uh-core) and the `goal_update` events.
- `slash_commands`: the builtin slash-command table every surface shares, with
  its pure parse/suggestion helpers.
- `skill_blocks`: the `<skill ...>` block parse shared by expansion and
  admission.
- `themes`: the bundled theme files (`unharnessed`/`dark`/`light`) as pure data,
  read by the session HTML exporter.
- `usage`, `memory_release`: small shared data helpers.
- `platform`: process identity and home-dir helpers (`platform::dirs::home_dir`:
  `HOME`, then `USERPROFILE` / `HOMEDRIVE`+`HOMEPATH` on Windows), and the
  executable's folder (`platform::dirs::executable_dir`, a link to the
  executable followed to the file).

## Non-goals

No provider logic, no session logic, no UI. Beyond pure data helpers and the
platform helpers, no behavior: a domain type that wants a method belongs in the
owning crate.

## Public API

Everything in this crate is deliberately `pub`: it is the cross-crate contract.
Unknown fields survive round-trips via catch-all maps so schema revisions stay
compatible.

## Depends on

serde, serde_json, thiserror, anyhow, tokio, libc (memory release). No workspace crates.
