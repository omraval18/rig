# Changelog

All notable changes to rig are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and rig uses [semantic versioning](https://semver.org/) (while it is `0.x`, minor versions may still change the config format).

## [Unreleased]

## [0.2.0] - 2026-10-31

### Added

- `ask_user`: the agent can ask a question with up to six options. The panel always ends with "Type something…", so you can give an answer of your own. Number keys pick an option; Esc skips the question; Ctrl-C skips it and stops nothing else.
- `/loop <objective>` (also `/goal`): keeps working toward an objective across turns. The model reports it complete or blocked through `update_goal`; the loop stops after 10 rounds, on Ctrl-C, or when you interrupt. `/loop` shows the goal, `/loop stop` drops it. Inspired by how Codex handles goals (Apache-2.0).
- Built-in web search for ChatGPT and OpenAI logins: the provider's own search tool is offered to the model, the same as Codex does. It is off for the other providers; no extra key is needed on the ChatGPT login.
- `/summary`: a short account of the session (goal, what was done, files changed, open items), shown without changing the history.
- `rig update`: checks the latest release and downloads it. `--check` only reports, `--off` and `--on` control automatic checks, and `--rollback` returns to the previous version.
- Automatic updates: at most once every 20 hours rig checks GitHub in the background. A newer release is downloaded, its signature and checksum are verified, it runs and reports the right version, and only then is it staged. The next start swaps it in and keeps the old binary as `rig.prev`. The footer says when an update is ready.
- Signed update checks: rig verifies a release's `SHA256SUMS` signature (minisign) before staging it. Released builds can update themselves once the release key is set up (see `RELEASING.md`); until then, `rig update --check` still reports new versions.
- Release workflow and CI definitions, with pinned actions, a tag-to-version check and a signature check before publishing. They are not on GitHub yet (they need a token with the `workflow` scope). See `RELEASING.md`.

### Changed

- Messages typed while the agent is working are now called the queue ("Enter to queue"), and go in before its next step, as before.

## [0.1.0] - 2026-10-31

First release.

### Added

- Terminal coding agent in one static binary: a full-window UI with `@` file mentions, `$` skills, steering while the agent works, and image attachments.
- Profiles: separate logins, API keys and settings under one `~/.rig`. Select with `--profile`, `RIG_PROFILE`, `rig profile use`, or `/profile` in the UI. `rig profile create|list|use|remove`.
- Saved API keys: `rig key set <provider>` stores a key per profile with hidden input. Choosing a provider with no key in `/provider` opens a masked box for it. Saved keys take precedence over environment variables.
- `rigd`: the same binary, starting every run at full access (`high`), with no approval prompts.
- Four autonomy levels (`read-only`, `low`, `medium`, `high`) with risk-gated commands and an approval prompt in the UI.
- Providers: ChatGPT subscription (`rig login codex`), Anthropic, OpenAI, xAI, OpenRouter, Ollama, and any OpenAI-compatible server.
- Sessions saved as JSON Lines files. `rig -c`, `rig --resume`, `/resume`, `/fork`, `/rename`, `/undo`.
- Context management: automatic compaction with an archive that can be searched with the `recall` tool.
- Skills loaded from `.rig/`, `.agents/` and `.claude/` folders, and `AGENTS.md` instructions.
- `install.sh`: builds rig from source and installs it, and `rigd`, to `~/.local/bin`.

### Changed

- Release binary optimised for size: about 3.2 MB, down from 4.3 MB.
- Disk writes reduced: a session file is created only on the first real message, unchanged settings are not rewritten, and prompt history is capped at 2,000 entries.
- The `@` file index stores each path once and matches case without a second copy.
