# tug for the browser

A Chrome and Edge extension that brings tug into the browser: the newest verification code (fill it
into a sign-in page with a right-click or **Alt+Shift+C**), search across texts and notifications,
send a text (or reply to one from search), the phone's status and its music controls. It's a companion to the tug app and does nothing
without it.

## How it reaches tug

```
extension ──native messaging──► bin\tug.exe (native host) ──named pipe──► tug.exe ──Bluetooth──► iPhone
```

- The browser starts `bin\tug.exe` with the extension's origin as its first argument and exchanges
  length-prefixed JSON over stdin/stdout (`src-tauri/crates/tug-bridge/src/native.rs` for the
  format, `src-tauri/crates/tug-cli/src/native_host.rs` for the loop).
- `tug.exe` makes each call over tug's existing developer bridge as a **browser** client. tug's
  master switch (Settings › Developer tools › Let AI tools use tug), its per-tool switches, its rate
  limits and its confirmation card all apply. The browser never sees the bridge token.
- Only these calls get through the host: `status`, `phone_status`, `get_latest_code`,
  `search_messages`, `media_control` and `send_text` (always through tug's card). File calls are
  refused.
- The installer registers the host for Chrome and Edge (`HKCU\Software\{Google\Chrome,Microsoft\Edge}\NativeMessagingHosts\dev.davejames.tug.browser`
  → `bin\browser-host.json`, from `src-tauri/windows/`). The portable exe doesn't, so the
  extension needs the installed tug.

## Permissions

`nativeMessaging`, `contextMenus`, `activeTab`, `scripting`, `storage`. No host permissions and no
content scripts: a page is touched only when you ask to fill a code on it.

`storage` is `chrome.storage.session` only (memory, gone when the browser closes): who the last
text from the popup went to and how it went, so the popup can say so after it closed for tug's
card. Never the message itself.

## Texts

The popup's **New text** (or a reply button on a search result) sends through tug's `send_text`,
and the host always asks for tug's card: nothing goes until you click Send in tug. The popup closes
when you click into tug, so the service worker holds the call open (`connectNative`, which also
keeps it alive) and keeps the outcome; the toolbar badge shows `…` while tug waits, then `✓` or `!`.

## Develop

No build step: the folder is the extension.

1. Install tug (setup exe), turn on Settings › Developer tools › Let AI tools use tug.
2. `chrome://extensions` → Developer mode → **Load unpacked** → this folder. The `key` in
   `manifest.json` pins the ID to `fdlhifdliipbdojnokcjdomknldbejbg`, the one the native host
   allows; an extension loaded with another ID gets "Access to the specified native messaging host
   is forbidden".
3. Preview any popup state without tug: `popup/popup.html?mock=connected` (also `search`, `copied`,
   `old`, `nocode`, `paused`, `connecting`, `disconnected`, `codesoff`, `notrunning`, `off`,
   `notinstalled`, `wrongid`, `loading`, `compose`, `textsent`, `textwaiting`, `textsoff`, `copyfail`;
   in `compose`, sending to "nobody" or "sam" shows tug's two answers about who it's to), served from any static server or inside the extension.

Tests:

```bash
node --test extension/test/format.test.mjs
```

```bash
node extension/test/page.e2e.cjs
```

On Windows, with tug installed and running, check the registration and one call through
`bin\tug.exe` exactly as the browser makes it, without a browser:

```powershell
powershell -ExecutionPolicy Bypass -File extension\test\host-check.ps1
```

The first runs in `npm run check`. The second (Fill code against sign-in pages shaped like real
ones, and the toast) needs Playwright with Chromium, which isn't a project dependency.

## Files

| Path | What |
|---|---|
| `manifest.json` | Manifest V3 |
| `service-worker.js` | The right-click item and the shortcut: get the code, fill it, show the toast |
| `lib/tug.js` | The one way to tug: a native message per call |
| `lib/format.js` | Pure helpers (code grouping, ages, error wording), unit-tested |
| `lib/page.js` | Injected into the page on a gesture: find the code field and fill it; the toast |
| `lib/mock.js` | Made-up answers for `?mock=` previews |
| `popup/`, `options/` | The toolbar popup and the options page |
| `styles/` | tug's design tokens (light and dark), the Lucide icon sprite |
| `fonts/`, `assets/`, `icons/` | Inter, EB Garamond, JetBrains Mono (OFL), tug's mark, the toolbar icons |
| `design/` | The design record: decisions and screenshots of every state (not shipped) |
| `CHROMEWEBSTORE.md` | Everything the store listing needs |
