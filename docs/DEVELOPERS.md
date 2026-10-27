# tug for developers: MCP server and the `tug` command

tug can hand your iPhone to the tools you code in: the newest 2FA code, a search of your texts
and notifications, CI and deploy notifications, a phone screenshot sent with Tugboat, what's
playing, and (only with a click from you) a text or files sent to the phone. It works through **`tug mcp`**, an MCP server
for AI tools, and the **`tug`** command for your terminal. Both are one small program that comes
with tug: `<tug's install folder>\bin\tug.exe`.

Everything is **off** until you turn on **Settings › Developer tools › Let AI tools use tug**.

## Find your install path

Settings › Developer tools shows the full path to `tug.exe` under **The tug command**, with a
**Copy path** button, and fills it into every setup snippet below. tug installs per user, so the
path is normally:

```text
%LOCALAPPDATA%\tug\bin\tug.exe        (C:\Users\<you>\AppData\Local\tug\bin\tug.exe)
```

If you chose another folder in the installer, use that folder's `bin\tug.exe`. Don't point AI tools
at tug's own `tug.exe` (the app, one folder up): that's the windowed app, not the command.

## Set up an AI tool

Settings › Developer tools has these with your real path filled in and a Copy button. Turn on
**Let AI tools use tug** first, then restart the AI tool after adding tug.

**Claude Code**, in a terminal:

```bash
claude mcp add --scope user tug -- "C:\Users\<you>\AppData\Local\tug\bin\tug.exe" mcp
```

**Codex**, in `~/.codex/config.toml`:

```toml
[mcp_servers.tug]
command = 'C:\Users\<you>\AppData\Local\tug\bin\tug.exe'
args = ["mcp"]
```

**Cursor**, in `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "tug": { "command": "C:\\Users\\<you>\\AppData\\Local\\tug\\bin\\tug.exe", "args": ["mcp"] }
  }
}
```

**VS Code**, in `mcp.json` (Command Palette › *MCP: Open User Configuration*):

```json
{
  "servers": {
    "tug": { "type": "stdio", "command": "C:\\Users\\<you>\\AppData\\Local\\tug\\bin\\tug.exe", "args": ["mcp"] }
  }
}
```

**ChatGPT desktop: not supported yet.** It only connects to MCP servers on the internet (remote
connectors), not local stdio ones like tug's, so it can't start `tug mcp`.

## Tools and switches

Once **Let AI tools use tug** is on, each switch below starts in its default; change any of them
in Settings › Developer tools › What AI tools can do. The same switches apply to the `tug` command.

| Tool | What it does | Switch (default) |
|---|---|---|
| `get_latest_code` | Newest verification code from the last 10 minutes, with sender, app and age | Verification codes (**off**: a code is a key to an account, so it's opt-in) |
| `search_messages` | Search texts and notifications (`query`, `limit` ≤ 50, `since`) | Search texts and notifications (on) |
| `recent_dev_notifications` | Notifications from GitHub (incl. Actions), Slack, Linear, Jira, Sentry, PagerDuty, Vercel, Netlify (`since`, default 24 h) | Developer notifications (on) |
| `list_tugboat_files` | Files received with Tugboat (`Pictures\Tugboat`, or `Downloads\Tugboat` while Windows Security refuses Pictures), newest first | Files from Tugboat (on) |
| `get_tugboat_file` | One of those by name; PNG/JPEG/GIF/WebP up to 3.5 MB come back as an image | Files from Tugboat (on) |
| `phone_status` | Connected or not, model, battery, now playing | Phone status (on) |
| `media_control` | `play`, `pause`, `toggle`, `next`, `previous` | Music controls (**off**) |
| `send_text` | Asks to text `to` (contact name or number) `message`: tug shows a card, and it's sent only if you click **Send** within 2 minutes. Returns `sent`, `declined`, `timed_out`, `cancelled` or `failed` | Send texts (**off**) |
| `send_file_to_phone` | Asks to send `paths` (full, or relative to the AI tool's folder) to the iPhone with Tugboat: tug shows a card, and on **Send** waits up to 2 minutes for the phone to download them. Returns `Delivered`, `Declined`, `Not confirmed`, `Waiting for the phone timed out` or `Cancelled` | Send files to your iPhone (**off**; also `tug boat`) |

`since` takes `30m`, `2h`, `1d`, `1 week` or an ISO time/date (UTC unless it has an offset).
Times come back as UTC ISO 8601. The developer-app list lives in one place:
`src-tauri/src/devtools/dev_apps.rs`.

## The confirmation card

A text from an AI tool or `tug text`, and files from `send_file_to_phone` or `tug boat`, are only
ever sent from tug's own card (the one exception is below):

- For a text it says "*<tool>* wants to text *<name>*", with the number and the exact message.
- For files it asks "Send report.pdf (2.1 MB) to your iPhone?" (or "Send 3 files (…)"), lists each
  file with its size and the folder it's in (so you can see it's *that* `notes.txt`, or that it's
  in `.ssh`), and says who's asking. Names are shown the way the phone will get them.
- Nothing is sent unless you click **Send** within **2 minutes**. After that the card closes and
  the tool is told `timed_out`; a click after the 2 minutes never sends.
- Focus starts on **Don't send**, and **Esc** means Don't send, so a stray Enter can't send.
- **Send** stays disabled for 1.5 s after the card appears and whenever tug's window isn't in
  front, so a click or key press meant for something else can't land on it.

**Send `tug text` without asking** (Settings › Developer tools › The tug command, off until you
turn it on, and only while **Send texts** is on) skips the card for `tug text` typed in a
terminal: tug sends it straight away. Everything else still gets the card: AI tools over MCP
always, `tug text` run by a script or program with no terminal (its stdin isn't a console), and a
name that only *contains* what you typed (`ann` for "Joanne"; a full name, the start of one like
`tay` for "Taylor", or a number is fine). It goes off by itself when Send texts, the master switch
or Revoke access turns things off.
That check is a hint, not proof: anything that can type in your terminals, including an AI agent
that runs commands in a real terminal window (Cursor, Copilot and others do), looks the same as
you, so only turn it on if that's fine with you. The rate limit (5 texts per 10 minutes) still applies.
- One card at a time, and an answer only counts for the card it was given on.
- A message longer than the box says so, so you can scroll through all of it before you send.
- The **Send texts** (or **Send files to your iPhone**) switch must be on; with it off the tool is
  told so and no card appears. Turning a switch off closes its card as not sent.
- At most 5 texts per 10 minutes, and 10 file requests a minute.

## The `tug` command

```text
tug code [--copy]          the newest code on stdout (--copy: also on the clipboard, kept out of history)
tug boat <file>...         send files to your iPhone with Tugboat: tug asks you first
tug boat - --name <name>   send what's piped in (up to 64 MB) as a file called <name>
tug text <name> "<msg>"    text someone: tug asks you first (unless Send tug text without asking is on)
tug status                 is tug running, the phone (with Phone status on), what's switched on
tug mcp                    the MCP server (your AI tool starts this)
```

Exit codes: `0` worked · `1` didn't (no code, not sent, a switch is off, phone not connected,
rate limited) · `2` typed wrong · `3` couldn't reach tug (not running, AI tools off).

**`tug boat` step by step.**

```text
tug boat report.pdf                  wait up to 2 minutes for the phone (the default)
tug boat a.png b.png --wait 300      wait up to 5 minutes (1–600 seconds)
tug boat report.pdf --no-wait        done once it's on offer (tug still holds it 2 minutes)
git diff | tug boat - --name changes.diff
```

1. The command checks every file first: regular, readable files only. A folder ("src is a
   folder"), a device or pipe (`NUL`, `\\.\pipe\…`) or a missing file stops it with exit `2`
   before tug is asked. Paths resolve against the current folder; a symbolic link counts as the
   file it points to, and that file's name is what's shown and sent.
2. tug checks them again (same rules, Tugboat's 1 GB limit, 64 MB for stdin) and shows its card.
   The terminal says `Check tug: Send or Don't send…`.
3. **Don't send**, or no answer in 2 minutes: nothing is offered, exit `1`.
4. **Send**: tug opens Tugboat if it's off and holds the files for the phone. With no Tugboat page
   open on the phone, the panel comes up with its code and the terminal says `Scan the code in
   tug to receive report.pdf on your iPhone…`; with the page open, `tap Get next to report.pdf`.
5. Once the phone has downloaded every file: `Delivered: your iPhone downloaded report.pdf.`,
   exit `0`. If the wait runs out first, tug stops offering the files (a file the phone is still
   fetching gets up to a minute more), and it's `Waiting for your iPhone timed out…`, exit `1`.
   Closing Tugboat, removing a file from it, or turning the switch off also ends it, exit `1`.

Progress goes to stderr and the outcome to stdout. What's piped in is saved to a file of the
command's own in your temp folder, read into tug's memory when you click Send, and deleted when
the command ends; tug never writes it to disk.

`send_file_to_phone` works the same way (always waiting 2 minutes) except that paths are used
exactly as given: an AI tool's bare name is never looked up across the PC.

**Naming files for `tug boat`.** A full or relative path is used exactly as given. In the
terminal (never for an AI tool), a bare name
that isn't a file in the current folder (`tug boat img_6060`) is looked up: same name in any
case, and with any extension when you didn't type one. tug looks in the current folder first,
then asks Windows Search's index for files with exactly that name under your user folder
(newest first, up to 50, at most 3 seconds), and if the index can't answer, takes a quick bounded
look (4 folders deep, up to 3 seconds) through Desktop, Downloads, Documents, Pictures
(Pictures\Tugboat first) and OneDrive, wherever Windows keeps them. Hidden and system folders,
junctions and symbolic links are skipped; OneDrive's cloud folders are looked through. One match
is used, and tug says which (`Using C:\…\IMG_6060.jpeg`); several are listed, newest first, and
nothing is sent (exit `2`), as is a full page of 50 from the index, since there may be more; none
says so. Names with `\`, `/`, `:`, wildcards or quotes are never looked up, and the
lookup never leaves your PC.

**Getting `tug` on your PATH.** The installer doesn't touch PATH. Settings › Developer tools ›
**Add tug to PATH** adds tug's `bin` folder (which holds only the command) to *your* PATH (HKCU,
no admin) on that click; **Remove from PATH** undoes it. Terminals opened afterwards know `tug`;
ones already open don't. Or call it by its full path. The `bin` folder keeps the command named
`tug.exe` without clashing with the app's own `tug.exe`, so nothing else needs to be on PATH.

## Troubleshooting

| What you see | What it means and what to do |
|---|---|
| **"tug isn't running. Open tug and try again."** | The tug app isn't open (or was quit from the tray). Open tug; it can stay hidden in the tray. |
| **"AI tools are turned off in tug…"** | **Let AI tools use tug** is off. Turn it on in Settings › Developer tools. |
| **"tug's Developer tools aren't set up yet…"** | Developer tools have never been switched on on this PC, so there's no token yet. Turn on Let AI tools use tug. |
| **"*Switch name*" is switched off in tug › Settings › Developer tools.** | That tool's switch is off (codes, music, texts and sending files start off). Turn it on if you want the tool to have it. |
| **"tug's access for AI tools was reset with Revoke access…"** | You pressed Revoke access. Restart the AI tool (or run the command again) to reconnect. |
| **"tug is already asking you about something else…"** | Another card (a text or files) is waiting for Send or Don't send. Answer it, then try again. |
| **"Too many … calls. Try again in N s."** | A per-tool rate limit (for example 60 reads a minute, 5 texts per 10 minutes). Wait and retry. |
| **"Something other than tug answered…"** | The pipe was answered by a program that couldn't prove it was tug. Quit and reopen tug. If it keeps happening, report it (see SECURITY.md). |
| **"AI tools can't reach tug right now"** in Settings | The bridge didn't start. Quit tug from the tray and open it again. |
| `tug` isn't recognised in a terminal | Use **Add tug to PATH**, then open a *new* terminal; or run it by its full path. |
| The AI tool doesn't list tug's tools | Check the path points at `bin\tug.exe`, restart the AI tool, and run `tug status` in a terminal to check tug answers. |
| ChatGPT desktop can't add tug | Not supported yet: it can't start local MCP servers. |

## Safety

- **Local only.** A Windows named pipe (`\\.\pipe\tug-bridge-<hash>`) owned by your Windows user,
  whose ACL lets only that user in; remote clients are refused. No network port.
- **A token, never sent.** tug keeps a random per-install token in `bridge.token` in its local
  data folder (`%LOCALAPPDATA%\<tug's app identifier>`; the identifier is in
  `src-tauri/tauri.conf.json`): readable by your user only, and labelled medium integrity with
  no-read-up, so sandboxed (low-integrity) programs running as you can't read it either. Each
  call, tug and the client prove they both hold it (HMAC over fresh nonces); the token itself
  never crosses the pipe.
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
  marked open-world; `send_text` and `send_file_to_phone` are marked destructive. The `tug` command strips escape
  sequences and invisible characters from other people's words before printing them.
- **Revoke access** writes a new token: AI tools already running get "access was reset" until
  they're restarted, and the Connected tools list starts over.
- **Texts and files are always confirmed** (see [The confirmation card](#the-confirmation-card)).
  A prompt-injected agent can be fooled into *asking* to push a file to your phone; it can't send
  one without your click, and the card shows the folder each file comes from.
- **Rate limits** per tool: 60 calls a minute by default, 30 for search and music, 20 for
  Tugboat files, 10 for `tug boat`, 5 texts per 10 minutes.
- **Logged without content:** tug.log gets the tool name, who asked, outcome and a count; for files,
  how many and their total size, never names or paths.
- **Off means off:** while the switch is off the pipe only waits and answers "off": no polling, no
  logging, nothing at startup beyond creating it.

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

See [ARCHITECTURE.md](ARCHITECTURE.md) for how this sits next to the rest of tug.

## Developing

- `npm run build:cli` builds the command into `src-tauri/binaries/tug-cli.exe`, which tug's
  bundle installs as `bin\tug.exe`. `npm run tauri dev` and `npm run build:release` run it for
  you; a debug build without it uses an empty stand-in (Settings then hides the setup).
- A test build with its own identifier gets its own data, pipe and token, and doesn't meet the
  installed tug: `npx tauri build --debug --no-bundle -c test.json`, where `test.json` is
  `{"identifier": "dev.example.tug-test"}`. Point the command at it with
  `TUG_APP_ID=dev.example.tug-test`. Seed its `advertise` setting to `false` first if your real
  tug is running, so the two don't compete for Bluetooth.
- Scripted check: start `tug.exe mcp`, send `initialize`, `notifications/initialized`,
  `tools/list`, then `tools/call` for each tool (one JSON-RPC message per line).
- UI work without the app: `npm run dev` with `/?devtools` (Settings › Developer tools) or
  `/?devconfirm` (the confirmation card).
