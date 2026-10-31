# Changelog

All notable changes to rig are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and rig uses [semantic versioning](https://semver.org/) (while it is `0.x`, minor versions may still change the config format).

## [Unreleased]

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
