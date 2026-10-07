# tug for developers: MCP server and the `tug` command

tug can hand your iPhone to the tools you code in: the newest 2FA code, a search of your texts
and notifications, CI and deploy notifications, a phone screenshot sent with Tugboat, what's
playing, and (only with a click from you) a text. It works through **`tug mcp`**, an MCP server
for AI tools, and the **`tug`** command for your terminal. Both are a small program that comes
with tug: `<tug's folder>\bin\tug.exe` (normally `%LOCALAPPDATA%\tug\bin\tug.exe`).

Everything is **off** until you turn on **Settings › Developer tools › Let AI tools use tug**.

## Set up an AI tool

Settings › Developer tools shows these with your real path filled in and a Copy button.

| Tool | Setup |
|---|---|
| Claude Code | `claude mcp add --scope user tug -- "C:\Users\you\AppData\Local\tug\bin\tug.exe" mcp` |
| Codex | `~/.codex/config.toml`: `[mcp_servers.tug]` · `command = 'C:\Users\you\AppData\Local\tug\bin\tug.exe'` · `args = ["mcp"]` |
| Cursor | `~/.cursor/mcp.json`: `{"mcpServers": {"tug": {"command": "C:\\...\\tug.exe", "args": ["mcp"]}}}` |
| VS Code | `mcp.json` (*MCP: Open User Configuration*): `{"servers": {"tug": {"type": "stdio", "command": "C:\\...\\tug.exe", "args": ["mcp"]}}}` |
| ChatGPT desktop | Not yet: it only connects to MCP servers on the internet (remote connectors), not local stdio ones. |

## Tools

| Tool | What it does | Switch (default once on) |
|---|---|---|
| `get_latest_code` | Newest verification code from the last 10 minutes, with sender, app and age | Verification codes (**off**: a code is a key to an account, so it's opt-in) |
| `search_messages` | Search texts and notifications (`query`, `limit` ≤ 50, `since`) | Search (on) |
| `recent_dev_notifications` | Notifications from GitHub (incl. Actions), Slack, Linear, Jira, Sentry, PagerDuty, Vercel, Netlify (`since`, default 24 h) | Developer notifications (on) |
| `list_tugboat_files` | Files received with Tugboat (`Pictures\Tugboat`), newest first | Files from Tugboat (on) |
| `get_tugboat_file` | One of those by name; PNG/JPEG/GIF/WebP up to 3.5 MB come back as an image | Files from Tugboat (on) |
| `phone_status` | Connected or not, model, battery, now playing | Phone status (on) |
| `media_control` | `play`, `pause`, `toggle`, `next`, `previous` | Music controls (**off**) |
| `send_text` | Asks to text `to` (contact name or number) `message`: tug shows a card, and it's sent only if you click **Send** within 2 minutes. Returns `sent`, `declined`, `timed_out`, `cancelled` or `failed` | Send texts (**off**) |

`since` takes `30m`, `2h`, `1d`, `1 week` or an ISO time/date (UTC unless it has an offset).
Times come back as UTC ISO 8601. The developer-app list lives in one place:
`src-tauri/src/devtools/dev_apps.rs`.

## The `tug` command

```text
tug code [--copy]          the newest code on stdout (--copy: also on the clipboard, kept out of history)
tug boat <file>...         send files to your phone with Tugboat (opens it in tug)
tug text <name> "<msg>"    text someone: tug asks you first, nothing sends by itself
tug status                 is tug running, the phone, what's switched on
tug mcp                    the MCP server (your AI tool starts this)
```

Exit codes: `0` worked · `1` didn't (no code, not sent, a switch is off, phone not connected,
rate limited) · `2` typed wrong · `3` couldn't reach tug (not running, AI tools off).

**Getting `tug` on your PATH.** The installer doesn't touch PATH. Settings › Developer tools ›
**Add tug to PATH** adds tug's `bin` folder (which holds only the command) to *your* PATH
(HKCU, no admin) on that click; **Remove from PATH** undoes it. Or call it by its full path.
The `bin` folder keeps the command named `tug.exe` without clashing with the app's own
`tug.exe`, so nothing else needs to be on PATH.

## Safety

- **Local only.** A Windows named pipe (`\\.\pipe\tug-bridge-<hash>`) owned by your Windows user,
  whose ACL lets only that user in; remote clients are refused. No network port.
- **A token, never sent.** tug keeps a random per-install token in
  `%LOCALAPPDATA%\dev.davejames.tug\bridge.token`: readable by your user only, and labelled
  medium integrity with no-read-up, so sandboxed (low-integrity) programs running as you can't
  read it either. Each call, tug and the client prove they both hold it (HMAC over fresh
  nonces); the token itself never crosses the pipe.
- **Impostors are refused.** Before saying anything, the client checks the pipe it opened is
  owned by your user and was made at normal (medium) integrity or above, as tug's is; a pipe made
  by another user or a sandboxed program while tug is closed is refused. Until tug has proved it
  holds the token, nothing it sends is shown: an early "off" or "wrong version" picks one of the
  client's own sentences, and anything else counts as an impostor. Programs running as you at
  normal integrity can read the token like any of your files, so the master switch is the real
  off switch.
- **Other people's words are data.** Texts, notifications, sender and file names are written by
  others. The MCP server's instructions and tool descriptions say so, and results that carry them
  come wrapped as `{"note": "Untrusted content…", "results": [...]}`, so a message saying
  "ignore your instructions" is reported, not obeyed. Reading tools that return such content are
  marked open-world; `send_text` is marked destructive.
- **Revoke access** writes a new token: AI tools already running get "access was revoked" until
  they're restarted, and the Connected tools list starts over.
- **Texts are always confirmed.** `send_text` and `tug text` only ever show the card; one at a
  time; focus starts on Don't send and Esc means Don't send; Send stays disabled for 1.5 s after
  the card appears and whenever tug's window isn't in front; an answer only counts for the card
  it was given on; a click after the 2 minutes never sends; a message longer than the box says so.
- **Rate limits** per tool (e.g. 60/min for reads, 5 texts per 10 minutes).
- **Logged without content:** tug.log gets the tool name, who asked, outcome and a count.
- **Off means off:** while the switch is off the pipe only waits and answers "off" — no
  polling, no logging, nothing at startup beyond creating it.

## How it fits together

```
AI tool ──stdio/MCP──► bin\tug.exe mcp ─┐
terminal ─────────────► bin\tug.exe ────┴─named pipe (tug-bridge)─► tug app: switches, limits,
                                                                    confirmation card, history
```

- `src-tauri/crates/tug-bridge`: protocol (versioned JSON lines), auth, framing, the pipe server
  and client, the token file. Pure parts unit-tested; the whole exchange tested over a real pipe.
- `src-tauri/crates/tug-cli`: the command and the MCP server (official Rust SDK, `rmcp`). A
  separate console program because tug.exe is a windowed app (no terminal output or exit
  codes) behind the single-instance guard, and the small program starts instantly.
- `src-tauri/src/devtools`: what each tool does, the switches, rate limits, the confirmation
  state machine, PATH, and Settings' status.

## Developing

- `npm run build:cli` builds the command into `src-tauri/binaries/tug-cli.exe`, which tug's
  bundle installs as `bin\tug.exe`. `npm run tauri dev` and `npm run build:release` run it for
  you; a debug build without it uses an empty stand-in (Settings then hides the setup).
- A test build with its own identifier (`npx tauri build --debug --no-bundle -c test.json`, where
  `test.json` is `{"identifier": "dev.davejames.tug.devtest"}`) gets its own data, pipe and
  token, and doesn't meet the installed tug; point the command at it with
  `TUG_APP_ID=dev.davejames.tug.devtest`. Seed its `advertise` setting to `false` first if your
  real tug is running, so the two don't compete for Bluetooth.
- Scripted check: start `tug.exe mcp`, send `initialize`, `notifications/initialized`,
  `tools/list`, then `tools/call` for each tool (one JSON-RPC message per line).
