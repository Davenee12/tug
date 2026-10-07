# Security policy

tug sits between someone's phone and their PC: it sees their notifications, texts, contacts and
calls. Security reports are taken seriously and handled before anything else.

## Reporting a vulnerability

**Please report privately, not in a public issue, pull request or discussion.**

1. Open the repository's **Security** tab and choose **Report a vulnerability** (GitHub private
   vulnerability reporting). Only the maintainer can see it.
2. Include the tug version (Settings › About), your Windows version, what you did, what happened,
   and what an attacker gains. A proof of concept helps but isn't required.
3. Don't include anyone's real messages, phone numbers or contacts. Use made-up data (555-01xx
   numbers, `example.com` addresses).

What to expect: an acknowledgement within a few days, a fix or a plan on the private advisory, and
credit in the advisory and CHANGELOG.md if you'd like it. Please give us a reasonable time to ship a
fix before talking about it publicly.

Only the latest release gets security fixes.

## In scope

- **Tugboat** (the phone ↔ PC transfer server, `src-tauri/src/tugboat/` and `src/tugboat-page/`):
  a plain-HTTP server on the PC's private LAN address while the Tugboat panel is open. Anything
  that lets another device on the network read or change transfers, bind to a session it didn't
  scan, write outside `Pictures\Tugboat`, get around the Mark-of-the-Web, or keep the server alive
  after it should stop.
- **The developer bridge** (`src-tauri/crates/tug-bridge/`, `src-tauri/crates/tug-cli/`,
  `src-tauri/src/devtools/`): a named pipe for the `tug` command and the MCP server. Anything
  that lets another Windows user, a sandboxed (low-integrity) process, or a remote machine use it;
  read or forge the bridge token; send a text without the person clicking **Send** on tug's
  card; turn on a switch the person left off; or get tool results while Developer tools are off.
  Prompt injection that gets an AI tool to make tug *act* (rather than just read) also counts.
- **Data at rest**: tug's history database, settings, logs, caches and the bridge token, all under
  the current user's profile. Anything that writes them somewhere other users can read, puts
  message content or secrets into logs or **Copy diagnostics** output, or stores the Spotify token
  outside Windows Credential Manager.
- **Parsing what the phone or network sends**: ANCS/AMS, OBEX/MAP/PBAP (vCards, bMessages,
  listings), Tugboat requests, Spotify and weather responses. A crash, hang or memory blow-up from
  malformed input is a valid report.
- **The installer and the app's own files**: anything that lets a non-admin process replace code tug
  will run.

## Out of scope

- Someone with full access to your unlocked Windows account. Programs running as you, at normal
  integrity, can read your files like you can; the Developer tools master switch is the real off
  switch.
- An active attacker on your Wi-Fi tampering with the first Tugboat page load. Tugboat runs over
  plain HTTP by design and protects against passive listening only; this is documented.
- What iOS shares over Bluetooth, and Bluetooth pairing itself (handled by Windows and iOS).
- The installer not being code-signed yet, and SmartScreen warnings about it.
- Third-party services (Spotify, Open-Meteo, Apple's App Store lookup) and their availability.
- AI tools doing what they're allowed to do once you've switched a tool on (for example, an
  AI tool reading your texts after you turned on **Search texts and notifications**).
- Denial of service that needs physical access to the PC or the phone.
- Reports from automated scanners with no demonstrated impact.
