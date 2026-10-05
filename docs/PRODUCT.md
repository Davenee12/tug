# tug — product direction

Runs alongside the stabilization sprints. Engineering (stable, correct, fast, tested) comes first;
product ideas found along the way are written down here and revisited once the foundation is solid.

## North star
> "Damn, this is actually really cool." — not "this has 47 features."

**Your phone, on your desk.** Simple on the surface, powerful underneath. A new user gets it in seconds;
the longer you use it, the more small things make you think "oh, that's nice", until you don't want to
go back to picking up your phone.

### Layers (progressive power)
1. **Immediate** — notifications, texts, media, battery. Nothing to configure.
2. **Useful** — Ctrl+K search, Ctrl+N new message, replies that clear the phone, keyboard everywhere.
3. **Powerful** — command mode, widgets, connectors, opt-in smarts.
4. **Expert** — custom shortcuts and layouts, never forced on anyone else.

### Rules
- Solves a real problem → makes tug meaningfully better → fits the identity → simplest version wins.
- A competitor having it is not a reason. Ask what problem it solves; solve that, maybe better.
- 10 small delightful details > 1 giant feature. Speed is a feature. Animation must mean something.
- Personalization is learned, not configured. Anything touching location, accounts or the PC is opt-in.
- "Would I miss this tomorrow?" and "Would I tell a friend?" — if neither, don't build it.
- New ideas use the proposal format below and are not built until approved.

### Principles we're stealing (not features)
| From | Principle | In tug |
|---|---|---|
| Wispr Flow | Near-zero interaction cost; works wherever you are | Reply without switching apps; nothing to set up after pairing |
| Raycast | Small composable actions behind one key | Ctrl+K grows from search into actions |
| Superhuman | Keyboard-first, "inbox zero" feeling | Clearing in tug clears the phone; caught-up state that feels earned |
| Linear | Speed and polish as the brand | Instant feedback, optimistic UI, no spinners on local data |
| Claude / AI-native | AI where it saves effort, invisible otherwise | Opt-in smarts (codes, suggestions), never a chatbot tab |

## Product opportunity backlog (keep ≤ 10)
Scores: value · frequency · delight · complexity. Timing: **Next** = after stabilization, **Later** =
after onboarding/settings. Shipped ideas keep their row with a ✅ status so the history stays honest;
the ≤10 cap counts only **active** (not-yet-built) ideas — currently 5 (#2 building, #5 built pending hardware test, #7, #8, #9).

| # | Idea | Problem | Experience | V/F/D/C | Call | Timing |
|---|---|---|---|---|---|---|
| 1 | **One-time codes** | Reading a 2FA code off the phone and retyping it | A code in a text/notification gets a Copy chip (and Ctrl+Shift+C); clears the notification after | H/H/H/Low | ✅ Shipped v0.5.5 | — |
| 2 | **Live texts (MAP notifications / MNS)** | Texts can take up to 8 s to appear; no "Sent" confirmation | Texts land the instant the phone gets them; your sends show "Sent" | H/H/M/Med | 🔨 Building (v0.5.7) | Now |
| 3 | **Ctrl+K actions** | Common actions need clicks through views | Type "tay running late" → send; "pause"; "clear all" | H/M/H/Med | ✅ Shipped v0.5.5 (more v0.5.6) | — |
| 4 | **Tray presence** | tug is invisible when minimized; no unread signal | Tray icon with unread count; click opens the latest conversation | H/H/M/Low | ✅ Shipped v0.5.5 (click → latest unread v0.5.6) | — |
| 5 | **Reply from the Windows pop-up** | Answering a text means opening tug | Type into the toast, Send; plus Mark read, Copy code, Call back, Clear on the pop-ups they fit | H/H/H/Med-High | 🔨 Built natively (WinRT toasts, not the plugin); needs a hardware test | v0.5.8 candidate |
| 6 | **Glance strip** (widgets, done small) | Glanceable info (phone battery, what's playing, next meeting, weather) is scattered | One quiet row above the Feed; drag a card bigger for more (weather → 7-day), smaller for less | M/H/H/Med | ✅ Partly — weather card shipped v0.5.4–v0.5.5; multi-card strip not built | — |
| 7 | **Welcome back** | Coming back to the PC, you don't know what you missed | After 30+ min away, the top of the Feed briefly summarizes who texted and what's waiting | M/M/H/Low-Med | Prototype | v0.5.8 candidate |
| 8 | **Suggested replies** | Typing the same short answers | 2–3 one-tap replies under a new text, learned from your own replies first, AI opt-in | M/M/M/Med | Consider later | Later |
| 9 | **Mini mode** | Want texts + media visible while working | Small always-on-top window with the latest conversation and controls | M/M/H/Med | Consider later | Later |
| 10 | **Delete conversations** | Old threads clutter the list | ✕ with undo; local only | M/L/L/Low | ✅ Shipped v0.5.5 | — |

### Considered and not building (and why)
- **Read receipts for texts you send** — not possible over Bluetooth: iOS never shares whether the
  other person read or received a message. Honest "Sent" comes with #2. Only a Mac's Messages database
  has this.
- **Lock the PC when the phone walks away** — Windows already does this (Dynamic Lock). Duplicate.
- **Focus-aware pop-ups** — Windows Focus/Do Not Disturb already holds tug's toasts. Duplicate.
- **Send later** — rare need, and it only works while the PC is on. Not worth the edge cases.
- **Pinned people** — the conversation list is already sorted by recency; revisit only if usage shows a need.
- **A widget platform / marketplace** — build the glance strip first; an ecosystem needs evidence.
- **Older text history and your own sent texts on a new install** — not possible over Bluetooth: the
  iPhone shares only its 10 newest incoming texts and an empty Sent folder (checked 2026-10-05 with
  `map_probe`). tug keeps history from the moment it's set up; conversations fill in as people text.

## Pairing facts learned on hardware (2026-10-05)
- Notifications (LE, via LightBlue) and texts (Classic, Windows › Add device) are two pairings. Pair
  notifications **first**, then texts: the other order broke the texts pairing in 2 of 3 runs. The setup
  wizard follows this order.
- The iPhone only shows Show Message Notifications / Sync Contacts after tug has asked for them, so tug
  checks every 2 s while they're pending (first 5 minutes, or while setup/Settings is open).

## Proposal format (for new ideas)
💡 **Name** · Problem · Why now · User value · Experience · Complexity · Product fit · Delight ·
Performance impact · Maintenance cost · Recommendation (Build / Prototype / Consider later / Don't
build) · Why.

## UX debt noticed (fix as part of sprints)
- First run still depends on the LightBlue app for the **notifications (LE)** pairing — the biggest gap
  between "project" and "product". v0.5.7 removes LightBlue from the **texts (Classic)** step (tug
  pairs it itself); the LE first-pair still needs it (see roadmap → Pairing without LightBlue).
- ~~Settings live in a side panel; a real Settings page is on the roadmap.~~ Shipped (v0.5.5).
