# Stabilization — Sprint 1 baseline & backlog

## Status after Sprint 4 (v0.5.4)
| Sprint | Release | Done |
|---|---|---|
| 2 | v0.5.2 | H1–H4, M1–M5 (each with a regression test, mutation-checked) |
| 3 | v0.5.3 | M6 migrations, M7 universal search, M8 store lifecycle, M9 drafts, M10 dialog focus; Vitest harness |
| 4 | v0.5.4 | Phone sync (clear on open, MAP mark-read), contact renames (aliases), accuracy-audit fixes, M11 measured and not needed (every UI query < 20 ms at 50k notifications), `cargo audit` clean |
| 5 | (pending) | CI on every PR; second bug hunt (3 reviewers: 3 High, 3 Med, 8 Low found and fixed, each verified first); `actor.rs` split into 6 modules (pure move, verified per function); lows: stale live session, app-name retry, toast bursts, listing `>`, reused send handles |

Lows already fixed along the way: Ctrl+N inside text fields, notification-permission re-prompt,
unbounded `ui.seen`, listing datetimes with a zone offset. The sections below are the original
Sprint 1 baseline, kept for reference.

Date: 2026-10-05 · Release under review: v0.5.1 · Method: baseline checks + four independent read-only
reviews (Bluetooth LE core, MAP/PBAP messaging, data/IPC, frontend), each finding re-checked against the
code before it entered this list.

## Health: 🟡 Functional, needs improvement

Core workflows work on real hardware (a recent iPhone): notifications, media, battery, message
read/send, contacts. No security issues or crash paths in the Bluetooth LE core. But there are real
data-integrity bugs (wrong recipient, wrong name, vanishing or merged items), one crash path in
messaging, and **zero frontend tests** around the logic that groups and merges conversations.

## Baseline

| Check | Result |
|---|---|
| Build (release exe + installer) | ✅ 8.2 MB exe, 3.6 MB installer |
| Type check (vue-tsc) | ✅ clean |
| Lint (clippy `-D warnings`) / format | ✅ clean |
| Rust unit tests | ✅ 47 pass — protocol, storage, parsing |
| Frontend tests | ❌ none (grouping, dedupe, names, badges untested) |
| npm audit | ✅ 0 vulnerabilities |
| Rust advisories (`cargo audit`) | ⚪ not run — tool not installed |
| Outdated | TypeScript 6 → 7 available (major; not taking blindly) |
| Secrets in repo | ✅ none |
| CSP / Tauri capabilities | ✅ minimal (`core:default`, `notification:default`) |
| Log growth | ✅ rotates (bounded) |
| Largest file | `ble/actor.rs` 1,443 lines (candidate for splitting in Sprint 4) |

## Backlog

### High — fix first (Sprint 2)
| # | Problem | Where | Failure |
|---|---|---|---|
| H1 | Two contacts with the same name merge into one conversation; the reply number is whoever texted last | `src/lib/format.ts` groupConversations | **Reply sent to the wrong person**, silently |
| H2 | Contact name learned from text alone; two people sending the same "ok" can swap names | `messages.rs` learn_contacts | Conversation labelled with the wrong person, sticky |
| H3 | OBEX response with declared length < 3 panics | `map/obex.rs` parse_response | Messaging thread dies until restart |
| H4 | A replayed notification whose detail fetch fails is swept as "cleared" | `ble/actor.rs` sweep_if_settled | Notifications vanish from the feed while still on the phone |

### Medium
| # | Problem | Where |
|---|---|---|
| M1 | Late ANCS Data Source reply (after timeout) is taken as the next request's reply | `ble/actor.rs` on_data_source |
| M2 | Reconnect reuses the link "generation", so stale events can pass the filter | `ble/actor.rs` connect/setup |
| M3 | Two identical notifications collapse on reconnect (content match re-claims a row) | `store.rs` upsert_notification |
| M4 | Identical texts deduped away: backend (same second / no timestamp) and frontend (not 1:1) | `messages.rs` insert_incoming, `format.ts` |
| M5 | MAP worker can hang forever: writes and connect have no timeout | `map/session.rs` |
| M6 | No schema versioning; the first column change would break existing installs | `store.rs` init |
| M7 | Search: new notifications leak into results; Messages sidebar is built from the search-filtered list | `stores/tug.ts`, `MessageThreads.vue` |
| M8 | Event listeners/shortcuts never torn down (double-fire on remount); startup events can be overwritten | `stores/tug.ts`, `App.vue` |
| M9 | Draft conversation row can linger after the first send | `MessageThreads.vue` |
| M10 | Dialogs don't trap focus; pairing dialog has no Esc / initial focus | `PairingDialog.vue`, `NewConversation.vue` |
| M11 | Synchronous DB commands may block the UI thread on large history (measure first) | `commands.rs` |

### Low
Notification-service status flaps after connect · live session not cleared when setup fails ·
app names never re-fetched after a failure · Control Point writes outside the request queue (document) ·
AMS truncation flag ignored · outgoing handle collision can leave a message "pending" · unbounded
history / `ui.seen` growth and no setting size cap · no length validation on send · Ctrl+N fires inside
text fields · notification-permission re-prompt and no toast rate limit · 11-digit numbers without `+` ·
phone-local times parsed in the PC's zone · UpdateInbox sent before navigating to `telecom/msg` ·
vCard quoted-printable soft breaks and bMessage QP names · `>` inside listing attributes · international
numbers without `+` don't merge.

## Plan
- **Sprint 2:** H1–H4 and M1–M5, each fixed at the root with a regression test.
- **Sprint 3:** frontend test harness (Vitest) for format.ts/store logic; M4, M6–M10; accuracy audit.
- **Sprint 4:** measure (startup, large history, rendering), M11, split `actor.rs`, Low items.
- Then onboarding/distribution track (see ROADMAP.md), then features.
