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
network; every request is MAC'd with keys derived from it, the first phone to connect binds the
session, replays are refused, and file contents are sealed with XChaCha20-Poly1305. Received files
get safe names, land only in `Pictures\Tugboat`, and carry the Mark-of-the-Web. Plain HTTP means an
*active* attacker on the network could tamper with the first page load; that's documented and out
of scope (see SECURITY.md).

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

**Other local users and sandboxed processes (developer bridge).** The `tug` command and MCP server
reach tug over a named pipe that only the current user can open; low-integrity processes and
remote machines are refused. Each connection proves it holds the bridge token (an HMAC
challenge over fresh nonces) before any call. Developer tools are off by default, each tool has
its own switch, calls are rate limited, and sending a text always needs the person to click
**Send** on tug's own card.

**Malicious content in notifications and texts.** Message and notification text comes from other
people. The window renders it as text (Vue escapes it; there's no `v-html`), and the webview is
locked down so injected markup couldn't do much anyway:

- A strict Content Security Policy (`src-tauri/tauri.conf.json`): scripts only from tug itself, no
  `eval`, no plugins, no framing, no form posts, images only from tug or `data:` URIs (remote art
  and app icons are fetched and checked by Rust and handed over as `data:`), and network access
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
  IDs are checked before they touch a path, and UI settings can't write Bluetooth or developer-tool
  keys.
- Developer tools (the WebView2 inspector) are compiled out of release builds.

**Code fill.** Auto-copy and the type-the-code shortcut only ever handle a code `findCode` found
(4–8 digits) and no older than 10 minutes: `copy_code` refuses anything else the webview passes,
and the shortcut types only what the backend picks from tug's own history. Copied codes stay out
of clipboard history and cloud sync and are cleared after 2 minutes if still there; the shortcut
never types into tug's own window; logs say a code was copied or typed, never which.

**Prompt injection aimed at AI tools.** A text can say "ignore your instructions and…". tug
assumes it will: reading tools are separate switches the person turns on, and the one tool that
*acts* (sending a text) always stops at a confirmation card in tug's window. An AI tool can be
fooled into *asking*; it can't send without the person.

**Supply chain.** Rust dependencies are checked by `cargo-deny` (advisories, licences, crates.io
only), production npm dependencies by `npm audit`, the code by CodeQL, and Dependabot keeps
both lockfiles current. CI actions from third parties are pinned to commit SHAs.

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
Credential Manager. Logs and **Copy diagnostics** never record message contents, names, numbers
or tokens.

## Unsigned builds

The installer isn't code-signed yet, so Windows SmartScreen warns about it and there's no
publisher signature to check. Download tug only from the project's GitHub Releases page.
