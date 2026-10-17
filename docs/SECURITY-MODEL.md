# Security model

A short threat model for tug: who it defends against, how, and what it deliberately doesn't try to
stop. To report a problem, see [SECURITY.md](../SECURITY.md).

tug runs as the signed-in Windows user, talks to one iPhone over Bluetooth, and handles that
phone's notifications, texts, contacts and calls. Everything it keeps stays on the PC.

## What tug protects against

**Nearby attackers on Bluetooth.** Pairing and link encryption are Windows' and iOS's job; tug
only talks to the paired device it chose, and treats everything the phone sends (ANCS/AMS,
OBEX/MAP/PBAP) as untrusted input. The parsers are pure, bounds-checked modules with tests, and
malformed input must fail safely rather than crash or hang tug.

**Other devices on the same Wi-Fi (Tugboat).** Tugboat listens only while its panel is open (or Tugboat
Run asked for the phone as a controller), on the PC's private LAN address. The QR code carries a secret after `#` that is never sent over the
network; the page takes it out of the address bar at once and keeps it only in memory (never in
web storage, so reloading the page means scanning again); every request is MAC'd with keys derived from it, the first phone to connect binds the
session, replays are refused, and file contents are sealed with XChaCha20-Poly1305. Received files
get safe names (never the shape of tug's own temp files, which are swept at the next start), land
only in `Pictures\Tugboat` (or `Downloads\Tugboat` when Windows Security's Controlled folder access
refuses Pictures to tug, which the panel says), and carry the Mark-of-the-Web. Devices that don't
hold the secret can't wear Tugboat down either: a connection that hasn't proved it holds the secret
is closed after its first answer, when every connection slot is taken the oldest such connection
makes way (so they can't lock the phone out), and refused requests and failed connections are
logged at most once a minute, with a count. Plain HTTP means an
*active* attacker on the network could tamper with the first page load; that's documented and out
of scope (see SECURITY.md).

**The shared clipboard (Tugboat).** The PC's clipboard is read only when the person presses Send
clipboard to iPhone (tray, Ctrl+K or the panel), never in the background, and goes out only over
Tugboat's existing channel: the same port, secret, MACs, binding and sealing, with no new network
destination. Only the bound phone gets it, at its next signed check-in. The PC holds it in memory
(a picture as PNG bytes, never written to disk) for at most 2 minutes; it's let go the moment the
phone acknowledges it (`POST /api/clip`, signed and sealed like any request; for a picture only
after the phone really fetched every chunk), dropped when the 2 minutes are up, replaced by a newer
press, and gone with the session. Text or pictures beyond Tugboat's limits (256 KB of text, 64 MB
or 24 megapixels for a picture) are refused, and the clipboard's bitmap is parsed by a bounds-checked
pure module (`tugboat/dib.rs`). In the other direction, text pasted on the phone goes onto the
clipboard kept out of Windows' clipboard history and cloud clipboard sync. Logs record lengths and
kinds only, never contents. The honest limit is Tugboat's: an active attacker who tampered with the
first page load could read what was sent to the phone or put text on the PC's clipboard (as could
anyone already able to read Tugboat's page).

**The game controller channel (Tugboat Run).** When the game asks for the phone as a controller,
the bound phone may `POST /api/pad`, under exactly Tugboat's rules: the same secret, a MAC on every
request, the first phone binds, replays refused, the body sealed to its request. On top of that:
inputs are refused unless the game has the channel open (and again the moment it closes); they're
rate limited (90 a second sustained, a burst of 30; a phone sends at most ~60); the body is read
only up to 104 bytes; and the plaintext must be exactly `{"steer": -100..=100, "boost": bool}`, plus
an optional latency probe (`"age"` and `"rtt"`, whole milliseconds 0–10000, only ever summarised
once per session in a debug log line), with any other field, type or range refused. An input that
lands after a newer one is ignored (the phone may have two in flight). The only
thing an input can change is the steering and boost the game reads (`tugboat/pad.rs`): it never
reaches files, the clipboard, the panel or anything else, and an integration test checks that. A
quiet phone (700 ms without an input) is treated as gone and the boat goes back to the keyboard,
which always works and always wins. The same honest limit applies: an active attacker who
tampered with the first page load could steer the boat.

**The guest seat (Tugboat Run, two players).** A friend's phone in a two-player race never gets the
session secret. The race shows a separate **Player 2 code** whose `#` fragment carries `g.` and a
guest secret derived from the session secret with HKDF-SHA256 (salt `tugboat/1`, info
`tugboat guest v1`, 16 bytes): one-way, so it can't be turned back into the session secret, and it
gives the guest phone keys of its own for signing and sealing. Tugboat checks every request against
the session's keys and then the guest's. Guest keys can sign exactly two requests, `GET /api/state`
(which tells the guest only whether there's a race: never files, text or the clipboard) and
`POST /api/pad` (steering seat 2, under the controller rules above); anything else is refused. A
guest key can never bind the session, even if it's the first to connect, so it can't become the
owner; it joins only while a two-player game asks for a second phone, one phone at a time (a third
is turned away), and the Player 2 code is shown only for as long as the game asks. A guest page
left open can't keep Tugboat running either: its check-ins don't count as activity for the
10-minute idle stop, only its steering does. The person's
own phone still binds with Tugboat's usual code exactly as before. Like the rest of Tugboat, it's only as safe as the first
page load: an active attacker on the network could steer seat 2.

**Other local users and sandboxed processes (developer bridge).** The `tug` command and MCP server
reach tug over a named pipe that only the current user can open; low-integrity processes and
remote machines are refused. Each connection proves it holds the bridge token (an HMAC
challenge over fresh nonces) before any call. Developer tools are off by default, each tool has
its own switch (`status` needs only the master switch, so it leaves the phone out unless Phone
status is on), calls are rate limited, and sending a text always needs the person to click
**Send** on tug's own card.

**Malicious content in notifications and texts.** Message and notification text comes from other
people. The window renders it as text (Vue escapes it; there's no `v-html`), and the webview is
locked down so injected markup couldn't do much anyway:

- A strict Content Security Policy (`src-tauri/tauri.conf.json`): scripts only from tug itself, no
  `eval`, no plugins, no framing, no form posts, images only from tug or `data:` URIs (remote art,
  album art and app icons are fetched and checked by Rust and handed over as `data:`; album art
  only from Apple's image servers over `https`), and network access
  only to the weather service. Inline `style` attributes are allowed (Vue writes static style
  attributes as markup); `<style>` elements are not.
- `Object.prototype` is frozen before tug's scripts run (`freezePrototype`), which blunts
  prototype-pollution bugs.
- The webview gets only the Tauri permissions its UI uses (`src-tauri/capabilities/default.json`):
  listening for events, reading the app version, and notification permission. It has no file
  system, shell, HTTP or window-control access. Window, tray, file picker and pop-ups are driven
  from Rust.
- tug's own commands validate what the webview passes in: `open_url` opens only `http(s)` links,
  Settings pages are an allowlist, Tugboat can only reveal files it saved this session, app-icon
  IDs are checked before they touch a path, album art is cached under a hash of the artist and
  album (the names never reach a path, and over-long ones aren't looked up), and UI settings can't
  write Bluetooth or developer-tool keys.
- Developer tools (the WebView2 inspector) are compiled out of release builds.

**Code fill.** Auto-copy and the type-the-code shortcut only ever handle a code `findCode` found
(4–8 digits) and no older than 10 minutes: `copy_code` refuses anything else the webview passes,
and the shortcut types only what the backend picks from tug's own history. Copied codes stay out
of clipboard history and cloud sync and are cleared after 2 minutes if still there; the shortcut
never types into tug's own window; logs say a code was copied or typed, never which.

**Hide previews while sharing or presenting.** On by default. To spot a sharing bar, tug reads
the title of every open browser window (Chrome, Edge, Brave, Vivaldi, Opera) and Teams window, so
page titles pass through it; they're compared on the spot and never kept or logged (the log names
only the kind of signal). It fails closed: if Windows doesn't answer in time, previews are hidden.
When hiding starts, the phone's pop-ups already on screen or in Action Center are taken back.

**Voice typing.** Off by default. While it's on, tug registers one global shortcut from a fixed
list and opens the microphone only between that shortcut's press and release (polled with
GetAsyncKeyState; no keyboard hook, so tug never sees other keys). Audio stays in memory for that
one hold and goes to Windows' offline SAPI recognizer, never to the network or a file; tug adds no
network access, download or CSP change for it. The text is typed only into the window that was in
front when the shortcut was pressed, after the shortcut's keys are let go; a spoken new line is
Shift+Enter only in a short list of prose and chat apps where that never sends (Notepad, WordPad,
Word, Outlook, OneNote, Teams, Slack, Discord) and a plain space everywhere else (code editors,
terminals, browsers), so dictation can't send, accept a suggestion or run a command by itself. Logs record lengths and durations, never the words. Anyone who can
speak near the PC while the person holds the shortcut can put words into the focused window, like
anyone at the keyboard.

**Prompt injection aimed at AI tools.** A text can say "ignore your instructions and…". tug
assumes it will: reading tools are separate switches the person turns on, and the one tool that
*acts* (sending a text) always stops at a confirmation card in tug's window. An AI tool can be
fooled into *asking*; it can't send without the person.

**Supply chain.** Rust dependencies are checked by `cargo-deny` (advisories, licences, crates.io
only), production npm dependencies by `npm audit`, the code by CodeQL, and Dependabot keeps
both lockfiles current. CI actions from third parties are pinned to commit SHAs.

**Release builds.** Releases are built by GitHub Actions (`.github/workflows/release.yml`) from a
version tag on a commit on `main`, after the same checks as every pull request, and land as a
*draft* that the owner publishes by hand. The workflow's token is read-only except in the one job
that creates the draft. Secrets (the updater signing key, SignPath's token) are only read on a real
release run, never on pull requests or dry runs, and never echoed. The build fails if a local path
survived in the exes, and the checksums and `latest.json` are made after any code signing, so they
describe the files people download. See [RELEASING.md](RELEASING.md).

**Automatic updates.** Release builds set up for them check GitHub only
(`https://github.com/Davenee12/tug/releases/latest/download/latest.json`, then the installer it
names), from Rust; the webview's CSP is unchanged and it has no updater permission. Every download
is verified by `tauri-plugin-updater` against the minisign public key compiled into tug
(`tauri.conf.json`) before it can run, and the signature must name the same version the manifest
announces (`requireSignedVersion`), so a tampered `latest.json` can't pair a new version number with
an older signed installer. Only strictly newer versions are accepted: downgrades are refused, in
the plugin's comparator and again before installing. A failed check, download or signature leaves
tug running as it was (logged, retried later). tug never restarts on its own: the person clicks
**Update** and **Restart now**, which waits while a call rings, Tugboat moves files or voice typing
is busy, or quits from the tray (whose item then says **Quit and update tug**). Without a real
public key in the build, nothing checks at all. The signing key's private half lives only in the
owner's offline backup and the repository's Actions secrets; anyone holding it could sign an update
every installed tug would accept, which is why it is never committed and never used outside a
release run. Losing it means installed copies can't update automatically any more (they'd need a
manual reinstall of a build with a new key).

## What tug doesn't protect against

- **Malware already running as you.** A program at normal integrity in your Windows session can
  read your files, your tug data and the bridge token just as you can. Turning Developer tools off
  is the real off switch for the bridge.
- **A compromised or unlocked phone.** tug mirrors what the phone shares; if the phone is someone
  else's to control, so is what tug shows and sends.
- **Someone at your unlocked PC.** They can open tug like you can.

## Data at rest

History (`tug.db`), settings, logs and caches live unencrypted under your Windows profile
(`%APPDATA%` and `%LOCALAPPDATA%`, see [ARCHITECTURE.md](ARCHITECTURE.md#storage)), protected by
Windows' per-user file permissions, not by tug. Use BitLocker or device encryption to protect
them from someone with the disk. The Spotify refresh token is the exception: it's kept in Windows
Credential Manager. Logs never record message contents, contact or device names, numbers or
tokens (devices are logged by a short tag). **Copy diagnostics** also masks phone numbers, emails,
Bluetooth addresses, the phone's name, and your Windows account name and profile folder.

**Start with Windows** is only ever your own entry (`HKCU\...\Run`), never one for every account on
the PC, so you can always turn it off without an administrator and the uninstaller removes it.
(Older versions could write an all-users entry if tug ran as administrator; tug moves that back to
you the next time it runs as administrator.)

## Unsigned builds

The installer isn't code-signed yet, so Windows SmartScreen warns about it and there's no
publisher signature to check. Download tug only from the project's GitHub Releases page, and
compare its SHA-256 with the checksums in the release notes. The release workflow is ready for
SignPath's free open-source signing and turns it on once SignPath approves the project. Automatic
updates don't depend on code signing: they're verified with tug's own update key.
