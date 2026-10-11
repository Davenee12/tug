# Contributing to tug

Thanks for helping. tug is a small project with a high bar for one thing above all: **it must tell
the truth**, both to the person using it and to the people reviewing your change. This guide covers
how to set up, how changes flow, and the rules that keep tug trustworthy.

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Found a security problem?
Don't open an issue: see [SECURITY.md](SECURITY.md).

## Set up

You need Windows 10 or 11 (tug is Windows-only: WinRT Bluetooth and an NSIS installer).

1. **Node.js 22** (LTS) and npm.
2. **Rust stable** via [rustup](https://rustup.rs), with `clippy` and `rustfmt`
   (`rustup component add clippy rustfmt`).
3. **Visual Studio Build Tools** with *Desktop development with C++*, including a **Windows 10/11
   SDK** (it provides `RC.EXE`, which the Rust build needs for tug's icon and manifest).
4. **WebView2 Runtime** (already on Windows 11 and up-to-date Windows 10).

Then:

```bash
npm install
npm run dev          # the UI in a browser with sample data, no phone or Bluetooth needed
npm run tauri dev    # the real app, with Bluetooth
npm run check        # everything CI runs; must pass before you push
```

`npm run check` builds the Tugboat phone page, then runs `vue-tsc`, Vitest, the build scripts'
tests (`node --test`), `cargo fmt --check`,
`cargo clippy -D warnings` and `cargo test` across the Rust workspace. If the Rust build fails
with an `RC.EXE` error, install the Windows SDK from the Build Tools installer, or run
`cargo clean -p tug --manifest-path src-tauri/Cargo.toml` and try again.

Most UI work needs no phone: `npm run dev` loads `src/lib/devMock.ts`, which plays scenarios from
the URL (`/?call`, `/?setup`, `/?devconfirm`, …). The full list is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#working-without-a-phone).

## How changes flow

1. **Fork** the repository and create a branch from `main` named for the change
   (`fix/sending-stuck`, `feat/quick-replies`, `docs/features`).
2. Make the change, with tests (see below). Keep one topic per pull request.
3. Run `npm run check` locally. It must pass; CI runs the same thing on Windows.
4. Open a **pull request against `main`** and fill in the template, including exactly how you
   tested it.
5. Review may ask for changes: push follow-up commits to the same branch rather than opening a new
   pull request.

For anything bigger than a bug fix, open an issue first so we can agree on the approach. New
features follow [docs/PRODUCT.md](docs/PRODUCT.md): stabilization comes first, and ideas start as a
short proposal (problem, user value, experience, complexity) before anyone builds them. Every fifth
version (0.5.15, 0.5.20, …) is bug fixing only.

## Rules that keep tug trustworthy

### Code

- **Logic goes in pure modules with tests.** Bluetooth can't be exercised without hardware, so
  protocol and decision logic lives in modules that take plain values and return plain values:
  `ancs.rs`, `ams.rs`, `store.rs`, `link_policy.rs`, `wedge.rs`, `wake.rs`,
  `map/contacts_watch.rs`, the `map/` parsers, `devtools/confirm.rs`, and `src/lib/*.ts` on the
  frontend. `ble/` and other WinRT code stay thin I/O that calls them. A bug fix comes with a test
  that fails without it.
- **`src/types/protocol.ts` mirrors the Rust serde types** (`state.rs`, `store.rs`, `ams.rs`,
  `devtools/`, and the others listed in ARCHITECTURE.md). Change both in the same commit; Rust
  uses `#[serde(rename_all = "camelCase")]` unless a type says otherwise.
- **Database changes are migrations.** Add a new entry to `MIGRATIONS` in `store.rs` (never edit
  an old one) and a test that upgrades from the previous version.
- **Every Windows call that can hang is time-limited.** Follow the patterns in `ble/winrt.rs`.
- Rust: `cargo fmt` and clippy with `-D warnings`, no new `unwrap()` on data from the phone or the
  network. TypeScript: strict, no `any` without a reason in a comment.
- Line endings are LF (`.gitattributes` enforces it).

### What people see

- **The app name is "tug", lowercase,** everywhere a person reads it, including at the start of a
  sentence.
- **Write for the person using tug, not for developers.** Plain words, no protocol names, error
  codes, test details or internal jargon in the UI. Say what happened and what to do next.
- **Never show something that isn't true.** No "Checking…" that can't finish, no "Sent" before the
  phone said so, no button that does nothing. If tug can't know, it says so.
- **Theme tokens only.** Colours, fonts and radii come from the `@theme` tokens in
  `src/style.css`: never inline a hex value in a component. Coral (`primary`) is only for the
  primary action, the serif (`headline`) is for display text, and device chrome uses the dark
  surfaces. Custom classes are Tailwind v4 `@utility` rules, not `@layer components`.
- Keyboard and screen readers matter: dialogs trap focus, Esc closes, controls have labels.

### Honesty about testing

- **Never claim hardware verification you didn't do.** If you didn't try a Bluetooth change on a
  real phone, say "not run against a phone" in the pull request and commit message. "Tests pass"
  and "works on an iPhone" are different claims.
- Say which phone model and iOS version you tried, and what you saw.

### Privacy

- **No personal data anywhere**: not in code, tests, fixtures, screenshots, logs you paste, or
  commit messages. Use made-up people, **555-01xx** phone numbers (for example `+1 555 0123`) and
  **`example.com`** email addresses. Don't commit your phone's name, your contacts, real messages,
  or paths containing your Windows user name.
- New logging must not record message contents, names, numbers or tokens. Copy diagnostics runs
  everything through the redaction in `diagnostics.rs`; keep it that way.

### Releases

The maintainer cuts releases. A release bumps the version in `package.json`, `package-lock.json`,
`src-tauri/Cargo.toml`, the workspace crates, `Cargo.lock` and `src-tauri/tauri.conf.json`; adds a
CHANGELOG.md entry; and adds a short, friendly entry to **`src/lib/whatsNew.ts`** (newest first,
the version in step with `package.json`; a test checks it). Preview it with `npm run dev` and
`/?whatsnew`. Release builds use `npm run build:release`, which strips local paths from the exe.

## The icon

`src-tauri/icons/source.svg` is the master; its rope geometry comes from `scripts/rope_geometry.py`.
Regenerate the PNG/ICO set with `npx tauri icon src-tauri/icons/source.svg -o src-tauri/icons`.

## Commit messages

- A short summary line in the imperative, describing the effect: "Texts: one slow text can't
  block every newer one".
- A body that explains **why**: what was wrong, what the person saw, why this fix is the right
  one, and the trade-offs. What you tested, and what you didn't ("Not run against a phone.").
- Reference issues with `Fixes #123` where it applies.

## Licence

tug is MIT licensed (see [LICENSE](LICENSE)). By contributing you agree that your contribution is
licensed under the same terms. The tug name and logo are the owner's; see the trademark note in the
README.
