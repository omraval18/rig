# rig

**A small, fast coding agent for your terminal.** One static Rust binary, no runtime, no async executor. Point it at a project and it reads the code, makes the smallest change that works, runs the checks, and reports back.

- **Stays on task.** Its prompt and tools are built around one loop: look, change minimally, verify, report.
- **Safe by default.** Four autonomy levels, from *ask before every edit* to *full access*. Commands are risk-gated: risky ones such as pushes and deletions need approval unless your level allows them, and every approval is shown in the UI.
- **Any model you have.** Your ChatGPT subscription, Anthropic, OpenAI, xAI, OpenRouter, Ollama, or any OpenAI-compatible server.
- **Profiles.** Keep a work and a personal account, or two API keys, side by side. Each profile has its own logins, keys and settings.
- **Full access on demand.** `rigd` is the same binary starting at full access, for trusted checkouts and containers.
- **Keys from inside the app.** Choose a provider with no key and a masked box asks for it. Pick **+ New profile…** to create another profile.
- **Small on disk.** A ~3 MB binary. Sessions appear only once you send a message, and settings are not rewritten when unchanged.
- **Local and inspectable.** Sessions are plain JSON Lines files on your disk. Network use is the model provider you chose, sign-in, and any web pages the agent fetches.

| | rig 0.1.0 |
|---|---|
| Binary | ~3.2 MiB |
| Startup | ~1.5 ms over bare process spawn |
| Memory | ~12 MB peak for a real request |

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/omraval18/rig/main/install.sh | sh
```

This builds rig from source with Cargo and installs `rig` to `~/.local/bin`. You need Rust, from [rustup.rs](https://rustup.rs). To read the script before running it:

```bash
curl -fsSL https://raw.githubusercontent.com/omraval18/rig/main/install.sh -o install.sh
less install.sh && sh install.sh
```

From a checkout, `./install.sh` builds that checkout instead. Set `RIG_INSTALL_DIR` to install somewhere else.

## Sign in

```bash
rig login codex                # ChatGPT subscription (browser sign-in)
rig key set anthropic          # or an API key, saved in the active profile (input hidden)
export ANTHROPIC_API_KEY=...   # or use the environment: also OPENAI_API_KEY, XAI_API_KEY, OPENROUTER_API_KEY
rig models                     # providers, default models, credential status
rig doctor                     # check the setup
```

Inside rig, `/provider` lists the providers. Choosing one that has no key opens a masked box for it.

## Use

```bash
rig                                   # interactive
rig ask "explain the failing test"    # one shot; --json for scripts
rig "fix the failing test"             # open the UI and send this as the first message (-- before text starting with a dash)
rig -m anthropic/claude-sonnet-5-5    # pick provider/model
rig -r high --auto medium             # reasoning effort, autonomy level
rig -c                                # continue the latest session in this directory
```

### Interactive

`rig` with no subcommand opens the full-window terminal UI described below.

| type | what happens |
|---|---|
| `/` | command popup with descriptions; arguments complete too (`/model gpt`, `/skills cr`) |
| `@` | file mention. Gitignore-aware fuzzy search over the workspace; exact name > prefix > word start > scattered, shallow paths first. `Tab` opens a folder, `Enter` mentions it. `@~`, `@.`, `@..` browse home, workspace, parent. Mentioned files (up to 24 KB each) are inlined, so the model doesn't need a `Read` call |
| `$` | skill. Lists `SKILL.md` skills from `.rig/ .agents/ .claude/` `skills/` in the project and home; `$name` activates one for that message |
| `Enter` while working | steers: the message is delivered after the current tool call, before the next model call |
| `Esc` | interrupts the turn (streaming and running commands stop immediately) |
| `Shift+Tab` / `Ctrl+P` | cycle permissions / model picker |
| `Alt+Enter`, `Ctrl+J`, trailing `\` | newline |

Commands: `/new` (`/clear`), `/resume`, `/fork`, `/rename`, `/compact`, `/model`, `/effort`, `/permissions`, `/allowlist`, `/undo`, `/skills [list|show|create|path]`, `/copy`, `/reasoning`, `/status`, `/settings`, `/provider`, `/profile`, `/login`, `/logout`, `/stats`, `/version`, `/help`, `/quit`. `/help` groups them.

### Full-window UI

rig takes over the whole terminal window (the alternate screen, like vim): the conversation fills the window, the prompt box is pinned to the bottom, and your shell is untouched when you quit. Only rows that changed are repainted, in one write, with the cursor hidden meanwhile, so resizing the window cannot leave stale or duplicated input boxes and the cursor never flickers around the screen. Text is wrapped at word boundaries (list items keep a hanging indent).

- **Scroll:** mouse wheel, PageUp / PageDown; End jumps back to the latest. Hold Shift (Option on macOS) to select text with the mouse, or turn mouse capture off in `/settings`. `/copy` copies the last reply.
- **`/new`, `/resume`, `/fork`** clear the window down to the new session's own messages.
- **Per-session settings:** every session records the model, effort and permissions it uses. `rig -c`, `rig --resume <id>` and `/resume` bring that session's own setup back, whatever other sessions used since (flags still win). New chats use what you used last.

### Images

Attach a picture three ways: press **Ctrl+V** (or run `/paste`) with an image on the clipboard, drag a file into the terminal, or mention it with `@screenshot.png`. Clipboard and dropped images show up as `[Image #1]` in the prompt and can sit anywhere in your text; delete the token to drop the image. PNG, JPEG, GIF and WebP work; big screenshots are downscaled first on macOS (`sips`). On Linux the clipboard needs `wl-paste` or `xclip`. All three providers send the image natively; the model must support vision.

The footer reads `model · effort <level> · permissions <level>`: reasoning effort and what rig may do without asking are separate settings (Shift+Tab changes the second).

### What you see while it works

- Each tool call starts with a status dot: a **pulsing yellow** `●` while it runs, a solid **green** one when it worked, a solid **red** one when it failed. Running calls live in the redrawn area at the bottom (printed text can't animate); they are printed with their final dot when they finish.
- Colour is used the same way everywhere: models are cyan, reasoning effort is violet, and access levels warm up with how much they allow (`ask` and `read-only` green, `low` blue, `medium` yellow, `high` red). The footer, `/status`, pickers and approval panel all follow this. The context meter stays plain until 50%, then turns yellow, then red at 80% (when auto-compaction starts).
- The prompt sits in a box (rule above and below) with the status line under it.

- Your messages are shaded cards; replies, tool activity and notes are separated by one blank line each. A resumed session replays the same way.
- Each tool call is one row with workspace-relative paths. What matters from the result follows it: the plan as a checklist, `exit 1 · FAILED (failures=1)` or `2 passed` for commands, `+1 -1` for edits, `3 files` / `no matches` for searches. Tools provide this through `Tool::summarize`.
- With no login, rig still opens: the footer says `not signed in`, your typed message is kept, and `/login` or `/provider` fix it from inside.
- If a request fails before the model answers (offline, rate limit, server error), the message is taken back out of the conversation and returned to the prompt, so Enter retries without sending it twice. The error says what happened in plain words.
- A session you opened and quit without typing is deleted; otherwise exit prints `rig -c` to continue it.
- The skill catalog sent to the model is capped (about 2k tokens, project skills first), so a large `~/.claude/skills` does not tax every request.

### Context engineering

- **Auto-compaction** at `auto_compact_percent` (default 80) of the model's context window. Your messages and the agent's final replies are kept verbatim. Older tool traffic is archived to JSONL and replaced by a checkpoint with `M#`/`T#` handles. Handles the summary invents are marked "(unverified)". The agent can look up or search the archive with the `recall` tool, so compacted details stay recoverable. After a compaction, it waits for real growth before compacting again.
- **Overflow recovery:** if the provider still rejects the context as too long, rig compacts once and retries.
- `/compact` does it on demand. `/fork` restarts from before any earlier prompt; `/undo` reverts the last `edit_files` batch.

### Autonomy

| level | runs without asking |
|---|---|
| `read-only` | reads, searches, and an allowlist of read-only shell commands |
| `low` (default) | + file edits, low-risk commands |
| `medium` | + installs, builds, local commits |
| `high` | everything |

`auto` is accepted for `low` and `full-access` for `high`; `ask` asks before every edit or command. Anything above the level is an approval prompt in the UI (yes / no / always; "always" rules persist in the profile's `allowlist.json`) and a denial in `rig ask`. The model declares a `risk` for every command, but it is only a claim: `risk_floor` raises it for pushes, deletions, `sudo`, `curl | sh`, and similar.

**Updates.** rig checks GitHub for a new release at most once every 20 hours, in the background. A newer release is downloaded and its signature and checksum are verified before anything else happens. The next start swaps it in, and the footer says so when an update is ready. Set `RIG_NO_UPDATE=1` to stop automatic checks, or run `rig update --off`. `rig update` checks and downloads now, `rig update --check` only reports, and `rig update --rollback` goes back to the previous version. Homebrew installs and development builds (under `target/`) are left alone: rig downloads nothing for them, and `brew upgrade rig` is the way to update a Homebrew install.

**`rigd` is full access.** It is the same binary under another name: `rigd` starts every run at `high`, so nothing asks for approval, including `rm`, pushes and `curl | sh`. Use it only in a checkout or container you don't mind the agent changing. `rig --auto <level>` still picks a level for `rig`.

### Profiles

A profile is a separate set of logins, API keys and settings: a work and a personal ChatGPT account, two Anthropic keys, different default models.

```bash
rig profile create work --use        # create it and use it from now on
rig login codex                      # signs the *active* profile in to ChatGPT
rig --profile personal login codex   # a different ChatGPT account in another profile
rig key set anthropic                # keys are per profile too
rig profile list                     # * marks the one in use
rig --profile personal               # one run in another profile (or RIG_PROFILE=personal)
rig profile use default
rig profile remove work              # asks first; -y to skip
```

In the UI, `/profile` lists profiles and `/profile <name>` (or picking one) restarts rig in it. The footer shows the profile name when it is not `default`.

- The `default` profile is `~/.rig` itself, so an existing setup keeps working untouched. Other profiles live in `~/.rig/profiles/<name>/`, each with its own `auth.json`, `keys.json` (mode 0600), `config.toml`, `state.json`, `allowlist.json`, prompt history and sessions.
- `~/.rig/config.toml` and `~/.rig/AGENTS.md` are shared; a profile's `config.toml` overrides the shared one, and a project's `.rig/config.toml` overrides both.
- Which profile: `--profile`, then `$RIG_PROFILE`, then `rig profile use`, then `default`.
- Which key: `api_key` in a config file, then the key saved in the profile, then the provider's environment variable. So a saved key wins over an exported one, and two profiles can use two accounts from the same shell.

### Config

`~/.rig/config.toml`, overridden by the profile's `config.toml`, then by `./.rig/config.toml`:

```toml
model = "chatgpt/gpt-6-sol"
effort = "medium"
autonomy = "low"
context_window = 272000       # override when a provider does not report one
auto_compact_percent = 80     # 10-80

[providers.my-vllm]          # any OpenAI-compatible server
kind = "chat"                # responses | anthropic | chat
base_url = "http://localhost:8000/v1"
keyless = true
```

**What you chose last wins.** Model, reasoning effort and permission level are saved to the profile's `state.json` as you change them (`/model`, `/effort`, `/permissions`, Shift+Tab), and the next chat starts with them. Order of precedence: command-line flag, then last used, then `config.toml`, then built-in defaults. A flag only applies to that run and does not overwrite what you saved. `rig ask` never inherits a saved permission level, so a `high` you picked once cannot leak into scripts. If the saved model's provider has no login any more, rig falls back to the config file.

`/settings` opens a list inside the UI: model, effort, permissions, reasoning summaries, auto-compact threshold, and **Remember last settings** (turn it off to always start from `config.toml`). Toggles apply immediately and are saved. `/settings files` shows which config files are loaded.

Project instructions: `AGENTS.md` files (repo root down to the working directory) and `~/.rig/AGENTS.md` are injected into the first message.

## Architecture

```
crates/
  rig-core      no network, no terminal. Message model, Provider + Tool traits, permission
                policy, agent loop (agent/), compaction (compact/), sessions, skills, @/$
                expansion, fuzzy ranking, slash-command registry, text/fs helpers.
  rig-tools     read_file, list_dir, find_files, search_text, run_command, edit_files (patch/),
                update_tasks, fetch_page, use_skill, recall; background file index for @.
  rig-provider  Auth + three wire adapters sharing one Endpoint: responses (OpenAI API /
                ChatGPT plan, chatgpt/), anthropic, chat (xAI/Grok, OpenRouter, Ollama, ...).
  rig-cli       the `rig` binary. app/ builds an agent from flags and config; tui/ is the event
                loop and state; slash/ one file per command group; view/ turns state into rows;
                transcript, screen and ansi do wrapping, scrollback and the diffed paint.
```

Dependencies point one way: `cli -> provider, tools -> core`. Adding a provider means one file implementing `Provider`; adding a tool means one file implementing `Tool`. Neither touches the agent loop.

Behaviors worth knowing (all covered by tests unless noted):

- **Easy on the disk.** A session file is created only once you send a message, so opening rig and quitting writes nothing. Settings files are replaced atomically and only when their content changed; re-recording the same session settings appends nothing; prompt history is trimmed once it grows past 2,000 entries. Session listing (`/resume`, `rig -c`) parses only titles and first prompts, not whole conversations.
- **Stateless turns.** Full history is replayed each turn (`store: false`), with encrypted reasoning items, and the session id is the prompt-cache key. Cached tokens show up in `rig ask --json`.
- **Scheduling.** Consecutive read-only tool calls run in parallel; an edit or command always runs alone and in order.
- **Denials are results.** A refused call returns an error to the model; it is never silently dropped.
- **Retries** happen only when nothing has been shown yet, so output is never duplicated.
- **`edit_files` is atomic.** Every hunk is applied in memory first; nothing is written unless all apply.
- **`run_command`** runs each call in a fresh `sh`, merges stdout/stderr, keeps head and tail within 32 KiB, and kills the whole process group on timeout.
- **`fetch_page`** refuses local/private hosts and credentialed URLs and validates every redirect. It does not resolve DNS, so it does not stop DNS rebinding.

## ChatGPT subscription

`rig login codex` runs its own PKCE flow, so rig owns a separate refresh-token chain. Sharing one chain with the Codex CLI would make the first refresh log the other out, because refresh tokens rotate. Tokens are stored in the profile's `auth.json` (`~/.rig/auth.json` for the default profile, mode 0600) in the same JSON layout as `~/.codex/auth.json`.

It uses the Codex CLI's public OAuth client id and the undocumented `chatgpt.com/backend-api/codex` endpoint. That can change without notice, and whether third-party use fits OpenAI's terms is your call.

## Not in 0.1.0

Subagents (`Task`), MCP, `WebSearch`, PDF input, hooks, Anthropic extended thinking, Cursor and Grok *subscription* login (Grok works today with an `XAI_API_KEY`), the Responses "custom tool" grammar for `edit_files` (it is sent as a normal function tool), and these fx commands: `/mcp`, `/feedback`, `/trace`, `/alias`, `/usage`, `/fast`, `/sound`, `/workspace`, `/shell`.

Testing: unit tests in every module (`cargo test`), plus pty-driven end-to-end scenarios against the live ChatGPT backend covering completion, pickers, mentions, skills, edit + undo, interrupt, approvals, steering, compaction + recall, resume/fork/new, paste and resize.

Known unverified: the browser sign-in has not been run end to end (OpenAI's authorize page is behind a Cloudflare challenge, so it cannot be tested headlessly). The Anthropic and Chat adapters are tested against a mock of their streaming formats, not live servers.

## Development

See [AGENTS.md](AGENTS.md) for the code conventions. `just check` runs format, clippy and the tests.
