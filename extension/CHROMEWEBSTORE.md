# Chrome Web Store listing: tug

> Last updated: 2026-11-26. Everything the Chrome Developer Dashboard asks for, in one place.
> Not shipped in the extension zip.

## Store listing

**Extension name:** tug

**Short description** (132 max):
Fill verification codes from your iPhone, search your texts and control its music, from the tug app on your Windows PC.

**Detailed description:**

```
Sign-in codes from your iPhone, filled in for you. tug for Chrome works with tug, the free Windows app that connects your iPhone to your PC over Bluetooth.

Fill a code without reaching for your phone. On a sign-in page, right-click the code box and choose "Fill code from iPhone", or press Alt+Shift+C. The newest verification code from your texts or notifications goes straight in, even into one-box-per-digit forms. No code box on the page? It's copied for you to paste instead.

See the newest code at a glance. Click the toolbar button to see the code, who sent it and how long ago, with a Copy button.

Search your texts and notifications from the toolbar, across what tug has on your PC.

Control your iPhone's music: play, pause and skip.

How to start:
1. Install tug for Windows (free) from https://davenee12.github.io/tug/ and connect your iPhone.
2. In tug, open Settings › Developer tools and turn on "Let AI tools use tug".
3. Click the tug button in your toolbar.

Private by design: the extension talks only to the tug app on your own computer. Nothing is sent to any server, there's no account and no tracking. It doesn't read the pages you visit; it touches a page only when you ask it to fill a code there. tug has a switch for each feature, and the extension can't send a text without you clicking Send in tug.

tug is free and open source. Questions or problems: https://github.com/Davenee12/tug/issues
```

**Category:** Productivity

**Single purpose:** Brings verification codes, texts and music controls from the user's iPhone, through the tug app on their PC, into the browser.

**Primary language:** English

## Graphics and assets

| Asset | Dimensions | Status | File |
|---|---|---|---|
| Store icon | 128×128 PNG | ✅ | `icons/icon-128.png` |
| Screenshot 1: the popup with a code | 1280×800 | ⬜ | |
| Screenshot 2: filling a code on a sign-in page | 1280×800 | ⬜ | |
| Screenshot 3: search | 1280×800 | ⬜ | |
| Small promo tile | 440×280 | ⬜ | |

Screenshots: use made-up data only (`popup/popup.html?mock=connected`, `?mock=search`), never a real
phone's texts.

## Permissions justification

| Permission | Type | Justification |
|---|---|---|
| `nativeMessaging` | permissions | The extension's only way to reach the tug app installed on the same PC, which holds the user's iPhone codes, texts and music. Nothing goes over the network. |
| `contextMenus` | permissions | Adds "Fill code from iPhone" to the right-click menu on text fields. |
| `activeTab` | permissions | When the user picks "Fill code from iPhone" or presses its shortcut, the code is put into the page they're on, and a short confirmation is shown there. No access to any page otherwise. |
| `scripting` | permissions | Used with activeTab, only on that gesture, to put the code into the field and show the confirmation. |

No `host_permissions`, no content scripts, no `tabs`, no storage.

## Privacy and data use

**Does the extension collect user data?** No. The extension shows the user their own codes, texts
and now playing, read from the tug app on the same computer at the moment they're shown. It keeps
nothing, sends nothing off the device, and has no analytics.

For the dashboard's form, "Personal communications" and "Authentication info" are *handled* on the
device (text snippets in search results, verification codes) but not collected or transmitted.

- [x] Data is not sold to third parties
- [x] Data is not used for purposes unrelated to the extension's core functionality
- [x] Data is not used for creditworthiness or lending purposes

**Privacy policy URL:** https://davenee12.github.io/tug/privacy/ (add a section for the extension
before submitting).

## Distribution

**Visibility:** Public. **Regions:** All.

## Developer info

**Publisher name:** _to fill in_ · **Contact email:** _to fill in_ ·
**Support URL:** https://github.com/Davenee12/tug/issues · **Homepage:** https://davenee12.github.io/tug/

## Before the first submission

- The store gives the extension its own ID. Add `chrome-extension://<store id>/` to
  `allowed_origins` in `src-tauri/windows/browser-host.json` (keep the development ID beside it) and
  ship a tug release with it, or the store build can't reach tug.
- Zip the `extension/` folder without `test/`, `design/` and this file.

## Version history

| Version | Date | Notes |
|---|---|---|
| 0.1.0 | — | First version: popup (phone, newest code, search, music), Fill code from iPhone. |
