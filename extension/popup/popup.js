// The toolbar popup: the phone, the newest verification code, search, now playing, and texting.
// It asks tug when it opens and every few seconds while it stays open, and keeps nothing
// afterwards (a text's outcome is kept by the service worker; see lib/texts.js).

import { tug, TugError } from "../lib/tug.js";
import {
  canReply,
  charCount,
  codeAge,
  composeProblem,
  explainError,
  explainSendError,
  groupCode,
  heldNowPlaying,
  initial,
  isOldCode,
  matchContacts,
  MAX_TEXT_CHARS,
  nowPlaying,
  phoneLine,
  phoneNotice,
  shortTime,
} from "../lib/format.js";
import { LAST_TEXT, MESSAGE_SEND_TEXT, lastTextNotice } from "../lib/texts.js";

const $ = (sel) => document.querySelector(sel);
const TICK_MS = 2500; // Status while music plays; everything else every other tick (5 s).
const ICONS = "../styles/icons.svg";

const mock = new URLSearchParams(location.search).get("mock");
let mockLast = null;
if (mock) {
  const { installMock, mockLastText } = await import("../lib/mock.js");
  installMock(mock);
  mockLast = mockLastText(mock);
}

let lastCode = null;
let permissions = null;
// tug › Settings › Developer tools › "Send texts from the browser extension without asking".
let sendsNow = false;
let composing = false;
let sending = false;

function show(view) {
  for (const el of document.querySelectorAll("[data-view]")) el.hidden = el.dataset.view !== view;
}

function icon(name) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "icon");
  const use = document.createElementNS(ns, "use");
  use.setAttribute("href", `${ICONS}#${name}`);
  svg.append(use);
  return svg;
}

const SCREEN_ICON = { install: "download", closed: "power", off: "shield-check" };

function showScreen(why) {
  $("[data-screen-art]").replaceChildren(icon(SCREEN_ICON[why.screen] ?? "circle-alert"));
  $("[data-screen-title]").textContent = why.title;
  $("[data-screen-body]").textContent = why.body;
  const actions = $("[data-screen-actions]");
  const hadFocus = actions.contains(document.activeElement);
  actions.replaceChildren();
  if (why.screen === "install") {
    const a = Object.assign(document.createElement("a"), {
      className: "btn btn-primary",
      href: "https://davenee12.github.io/tug/",
      target: "_blank",
      textContent: "Get the latest tug",
    });
    actions.append(a);
  } else {
    const retry = Object.assign(document.createElement("button"), { className: "btn btn-secondary", textContent: "Try again" });
    retry.prepend(icon("refresh-cw"));
    retry.addEventListener("click", () => void refresh());
    actions.append(retry);
    if (hadFocus) retry.focus(); // Pressing it again from the keyboard keeps working.
  }
  $("[data-pill]").hidden = true;
  $("[data-new-text]").hidden = true;
  composing = false;
  show("screen");
}

// Now playing, with what a music button just asked for shown before the phone reports it.
let mediaHold = null;
let shownNp = null;
let phoneNp = null; // What the phone last said, before any hold.
let holdTimer;

function paintNowPlaying(np) {
  phoneNp = np;
  const held = heldNowPlaying(np, mediaHold, Date.now(), shownNp);
  mediaHold = held.hold;
  shownNp = held.np;
  const strip = $("[data-np]");
  strip.hidden = !shownNp;
  strip.classList.toggle("pending", held.pending);
  if (!shownNp) return;
  $("[data-np-title]").textContent = shownNp.title;
  $("[data-np-sub]").textContent = shownNp.subtitle;
  $("[data-np-icon]").setAttribute("href", `${ICONS}#${shownNp.playing ? "pause" : "play"}`);
}

function renderPhone(phone, perms) {
  const pill = $("[data-pill]");
  pill.hidden = !phone;
  const notice = $("[data-notice]");
  notice.hidden = true;
  if (phone) {
    const p = phoneLine(phone);
    pill.dataset.state = p.state;
    $("[data-pill-name]").textContent =
      p.state === "connected" ? p.label : p.state === "connecting" ? "Connecting…" : p.state === "none" ? "No iPhone yet" : "iPhone offline";
    const battery = $("[data-pill-battery]");
    battery.hidden = !(p.state === "connected" && phone.battery_percent != null);
    $("[data-pill-pct]").textContent = `${phone.battery_percent}%`;

    const n = phoneNotice(phone);
    if (n) {
      notice.hidden = false;
      notice.className = `notice ${n.tone}`;
      $("[data-notice-icon]").setAttribute("href", `${ICONS}#${n.icon}`);
      $("[data-notice-title]").textContent = n.title;
      $("[data-notice-body]").textContent = n.body;
    }
  }
  paintNowPlaying(phone && perms?.media !== false ? nowPlaying(phone) : null);
}

function renderCode(result, error) {
  const card = $("[data-code]");
  const empty = $("[data-code-empty]");
  if (error) {
    lastCode = null;
    card.hidden = true;
    empty.hidden = false;
    const why = explainError(error.code, error.message);
    $(".empty-title").textContent = error.code === "not_found" ? "No recent codes" : why.title;
    $("[data-code-empty-body]").textContent =
      error.code === "not_found" ? "Sign-in codes texted to your iPhone show up here, ready to copy." : why.body;
    return;
  }
  const changed = lastCode?.code !== result.code;
  lastCode = result;
  card.hidden = false;
  empty.hidden = true;
  card.classList.toggle("old", isOldCode(result.age_seconds));
  $("[data-code-value]").textContent = groupCode(result.code);
  $("[data-code-age-text]").textContent = codeAge(result.age_seconds);
  $("[data-code-initial]").textContent = initial(result.from || result.app);
  $("[data-code-from]").textContent = result.from || result.app;
  $("[data-code-app]").textContent = result.from && result.app && result.app.toLowerCase() !== result.from.toLowerCase() ? result.app : "";
  if (changed) resetCopy();
}

function resetCopy() {
  $("[data-copy-note]").hidden = true;
  const btn = $("[data-copy]");
  btn.classList.remove("done");
  btn.querySelector("use").setAttribute("href", `${ICONS}#copy`);
  btn.querySelector("span").textContent = "Copy";
}

// One refresh at a time: the timer, the music buttons and Try again can all ask at once, and an
// older answer landing after a newer one would show stale state. Asks made meanwhile run once
// more when it's done (with the code too if any of them wanted it).
let refreshing = null;
let againWithCode = null;

function refresh({ code = true } = {}) {
  if (refreshing) {
    againWithCode = Boolean(againWithCode) || code;
    return refreshing;
  }
  refreshing = (async () => {
    try {
      let withCode = code;
      for (;;) {
        await refreshOnce(withCode);
        if (againWithCode === null) break;
        withCode = againWithCode;
        againWithCode = null;
      }
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

async function refreshOnce(withCode) {
  let status;
  try {
    status = await tug.status();
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    if (why.screen) return showScreen(why);
    status = null;
  }
  permissions = status?.permissions ?? permissions;
  if (status) sendsNow = status.sends_without_asking === true;
  renderPhone(status?.phone ?? null, status?.permissions);
  if (withCode) {
    try {
      renderCode(await tug.latestCode(false));
    } catch (e) {
      const err = e instanceof TugError ? e : new TugError("failed", e.message);
      const why = explainError(err.code, err.message);
      if (why.screen) return showScreen(why);
      renderCode(null, err);
    }
  }
  $("[data-new-text]").hidden = composing;
  if (composing) updateCompose();
  show(composing ? "compose" : "ready");
}

let copyTimer;
$("[data-copy]").addEventListener("click", async () => {
  const btn = $("[data-copy]");
  const note = $("[data-copy-note]");
  clearTimeout(copyTimer);
  resetCopy();
  try {
    // tug puts it on the clipboard itself, kept out of clipboard history and sync. It answers
    // with the code it copied, which may be newer than the one on screen.
    const r = await tug.latestCode(true);
    renderCode(r);
    if (!r?.copied) {
      note.textContent = "tug couldn't put it on the clipboard. Try again, or type it in.";
      note.hidden = false;
      return;
    }
    btn.classList.add("done");
    btn.querySelector("use").setAttribute("href", `${ICONS}#check`);
    btn.querySelector("span").textContent = "Copied";
    copyTimer = setTimeout(resetCopy, 2400);
  } catch (e) {
    const err = e instanceof TugError ? e : new TugError("failed", e.message);
    const why = explainError(err.code, err.message);
    if (why.screen) return showScreen(why);
    note.textContent = `${why.title}. ${why.body}`;
    note.hidden = false;
  }
});

// Music: show what was pressed at once, then ask the phone a few times as it catches up.
const MEDIA_HOLD_MS = { toggle: 3000, skip: 4000 };
const MEDIA_FOLLOW_UP_MS = [150, 600, 1500, 3000];

for (const btn of document.querySelectorAll("[data-media]")) {
  btn.addEventListener("click", async () => {
    // aria-disabled, not disabled: a disabled button drops keyboard focus to the page.
    if (btn.getAttribute("aria-disabled") === "true") return;
    const action = btn.dataset.media;
    if (shownNp) {
      mediaHold =
        action === "toggle"
          ? { kind: "toggle", playing: !shownNp.playing, until: Date.now() + MEDIA_HOLD_MS.toggle }
          : { kind: "skip", title: shownNp.title, until: Date.now() + MEDIA_HOLD_MS.skip };
      paintNowPlaying(phoneNp ?? shownNp);
      // If the phone never comes round, stop pretending once the hold runs out.
      clearTimeout(holdTimer);
      holdTimer = setTimeout(() => paintNowPlaying(phoneNp), MEDIA_HOLD_MS[action === "toggle" ? "toggle" : "skip"] + 50);
    }
    btn.setAttribute("aria-disabled", "true");
    try {
      await tug.media(action);
    } catch {
      // The phone didn't take it: drop what was shown and let the next answer say how it is.
      mediaHold = null;
    } finally {
      btn.removeAttribute("aria-disabled");
    }
    for (const ms of MEDIA_FOLLOW_UP_MS) setTimeout(() => void refresh({ code: false }), ms);
  });
}

// Search, as you type (tug allows 30 searches a minute, so a pause before each).
const input = $("[data-search]");
const list = $("[data-hits]");
const note = $("[data-search-note]");
let searchTimer;
let searchSeq = 0;

function highlighted(text, query) {
  const frag = document.createDocumentFragment();
  const lower = text.toLowerCase();
  const q = query.toLowerCase();
  let i = 0;
  for (let at = lower.indexOf(q); q && at !== -1; at = lower.indexOf(q, i)) {
    frag.append(text.slice(i, at), Object.assign(document.createElement("mark"), { textContent: text.slice(at, at + q.length) }));
    i = at + q.length;
  }
  frag.append(text.slice(i));
  return frag;
}

function renderHits(hits, query) {
  list.replaceChildren(
    ...hits.map((h) => {
      const li = document.createElement("li");
      li.className = "hit";
      const avatar = Object.assign(document.createElement("span"), { className: "avatar", textContent: initial(h.from) });
      avatar.setAttribute("aria-hidden", "true");
      const who = Object.assign(document.createElement("span"), { className: "hit-who" });
      // The app only when it adds something ("OpenTable OpenTable" says nothing twice).
      const app = h.app && h.app.toLowerCase() !== String(h.from).toLowerCase() ? h.app : "";
      who.append(Object.assign(document.createElement("strong"), { textContent: h.from }), app);
      const end = Object.assign(document.createElement("span"), { className: "hit-end" });
      end.append(Object.assign(document.createElement("span"), { className: "hit-time", textContent: shortTime(h.time) }));
      if (canReply(h)) {
        li.classList.add("can-reply");
        const reply = Object.assign(document.createElement("button"), { className: "icon-btn hit-reply", title: `Reply to ${h.from}` });
        reply.setAttribute("aria-label", `Reply to ${h.from}`);
        reply.append(icon("reply"));
        reply.addEventListener("click", () => openCompose(h.from));
        end.append(reply);
      }
      const text = document.createElement("span");
      text.className = "hit-text";
      text.append(highlighted(h.text, query));
      li.append(avatar, who, end, text);
      return li;
    }),
  );
  list.hidden = hits.length === 0;
  note.hidden = hits.length !== 0;
  note.textContent = hits.length ? "" : `Nothing matches “${query}”.`;
}

input.addEventListener("input", () => {
  clearTimeout(searchTimer);
  // Any answer still on its way is for a search that's gone, even if the box is now empty.
  const seq = ++searchSeq;
  const query = input.value.trim();
  if (query.length < 2) {
    list.hidden = true;
    note.hidden = true;
    return;
  }
  searchTimer = setTimeout(async () => {
    try {
      const hits = await tug.search(query);
      if (seq === searchSeq) renderHits(Array.isArray(hits) ? hits : [], query);
    } catch (e) {
      if (seq !== searchSeq) return;
      list.hidden = true;
      note.hidden = false;
      note.textContent = explainError(e.code, e.message).body;
    }
  }, 350);
});

// "/" jumps to search, as in most apps with one search box, unless you're typing somewhere.
document.addEventListener("keydown", (e) => {
  const typing = e.target instanceof HTMLElement && (e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName));
  if (e.key === "/" && !typing && !composing && !e.ctrlKey && !e.metaKey && !e.altKey) {
    e.preventDefault();
    input.focus();
  }
});

// The last text sent from here, as the service worker kept it.
const textStore = globalThis.chrome?.storage?.session ?? null;

async function readLastText() {
  if (!textStore) return mockLast;
  try {
    return (await textStore.get(LAST_TEXT))[LAST_TEXT] ?? null;
  } catch {
    return null;
  }
}

async function clearBadge() {
  try {
    await chrome.action.setBadgeText({ text: "" });
  } catch {
    // Previewed outside the extension.
  }
}

async function forgetLastText() {
  mockLast = null;
  try {
    await textStore?.remove(LAST_TEXT);
  } catch {
    // Gone with the browser session anyway.
  }
  await clearBadge();
}

const TEXT_NOTICE_ICON_SPIN = "loader-circle";

async function renderLastText() {
  const n = lastTextNotice(await readLastText());
  const el = $("[data-text-notice]");
  el.hidden = !n;
  if (!n) return;
  el.className = `notice ${n.tone}${n.icon === TEXT_NOTICE_ICON_SPIN ? " spinning" : ""}`;
  $("[data-text-notice-icon]").setAttribute("href", `${ICONS}#${n.icon}`);
  $("[data-text-notice-title]").textContent = n.title;
  const body = $("[data-text-notice-body]");
  body.textContent = n.body;
  body.hidden = !n.body;
  $("[data-text-notice-dismiss]").hidden = !n.dismissable;
  if (!n.waiting) void clearBadge(); // Seen: the badge has said its piece.
}

$("[data-text-notice-dismiss]").addEventListener("click", async () => {
  await forgetLastText();
  $("[data-text-notice]").hidden = true;
});

try {
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "session" && LAST_TEXT in changes) void renderLastText();
  });
} catch {
  // Previewed outside the extension.
}

// Compose: a new text, or a reply from search.
const form = $("[data-compose]");
const toInput = $("[data-to]");
const msgInput = $("[data-message]");
const sendBtn = $("[data-send]");
const countEl = $("[data-count]");
const errEl = $("[data-compose-error]");
const noteEl = $("[data-compose-note]");
const waitingEl = $("[data-compose-waiting]");

// Who to text: the phone's contacts as a dropdown under To, fetched once per popup.
const contactsEl = $("[data-contacts]");
const contactsToggle = $("[data-contacts-toggle]");
const SHOW_CONTACTS = 150; // More than this and typing a few letters finds them faster.
let contactList = null;
let contactsLoad = null;
let shownContacts = [];
let activeContact = -1;
let showAll = false; // Opened with the arrow: everyone, whatever's in the field.

function loadContacts() {
  contactsLoad ??= tug
    .contacts()
    .then((list) => (Array.isArray(list) ? list.filter((c) => c && typeof c.name === "string" && c.name) : []))
    .catch(() => []) // Switched off, or a tug from before the picker: the field still takes a name.
    .then((list) => {
      contactList = list;
      contactsToggle.hidden = list.length === 0;
      if (document.activeElement === toInput) openContacts();
    });
  return contactsLoad;
}

function highlightedName(name, query) {
  const frag = document.createDocumentFragment();
  const q = query.trim().toLocaleLowerCase();
  const at = q ? name.toLocaleLowerCase().indexOf(q) : -1;
  if (at === -1) {
    frag.append(name);
    return frag;
  }
  frag.append(name.slice(0, at), Object.assign(document.createElement("mark"), { textContent: name.slice(at, at + q.length) }), name.slice(at + q.length));
  return frag;
}

function renderContacts() {
  const query = showAll ? "" : toInput.value;
  const matches = matchContacts(contactList ?? [], query);
  shownContacts = matches.slice(0, SHOW_CONTACTS);
  if (activeContact >= shownContacts.length) activeContact = shownContacts.length - 1;
  const items = [];
  const header = (text) => Object.assign(document.createElement("li"), { className: "group", textContent: text, role: "presentation" });
  const grouped = !query.trim();
  shownContacts.forEach((c, i) => {
    if (grouped && i === 0 && c.recent) items.push(header("Recent"));
    if (grouped && !c.recent && (i === 0 || shownContacts[i - 1].recent)) items.push(header("All contacts"));
    const li = document.createElement("li");
    li.className = `contact${i === activeContact ? " active" : ""}`;
    li.id = `contact-${i}`;
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", String(i === activeContact));
    const avatar = Object.assign(document.createElement("span"), { className: "avatar", textContent: initial(c.name) });
    avatar.setAttribute("aria-hidden", "true");
    const name = Object.assign(document.createElement("span"), { className: "contact-name" });
    name.append(highlightedName(c.name, query));
    li.append(avatar, name);
    // mousedown, not click: keep focus in the field so it doesn't close first.
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      pickContact(c.name);
    });
    items.push(li);
  });
  if (matches.length > shownContacts.length) {
    items.push(Object.assign(document.createElement("li"), { className: "more", role: "presentation", textContent: `Type to find the other ${matches.length - shownContacts.length}` }));
  }
  contactsEl.replaceChildren(...items);
  const open = shownContacts.length > 0;
  contactsEl.hidden = !open;
  toInput.setAttribute("aria-expanded", String(open));
  if (open && activeContact >= 0) {
    toInput.setAttribute("aria-activedescendant", `contact-${activeContact}`);
    $(`#contact-${activeContact}`)?.scrollIntoView({ block: "nearest" });
  } else {
    toInput.removeAttribute("aria-activedescendant");
  }
}

function openContacts() {
  if (!contactList?.length) return;
  renderContacts();
}

function closeContacts() {
  showAll = false;
  contactsEl.hidden = true;
  activeContact = -1;
  toInput.setAttribute("aria-expanded", "false");
  toInput.removeAttribute("aria-activedescendant");
}

function pickContact(name) {
  toInput.value = name;
  closeContacts();
  updateCompose();
  msgInput.focus();
}

toInput.addEventListener("focus", openContacts);
toInput.addEventListener("blur", closeContacts);
toInput.addEventListener("input", () => {
  showAll = false;
  activeContact = toInput.value.trim() ? 0 : -1;
  openContacts();
});
toInput.addEventListener("keydown", (e) => {
  const open = !contactsEl.hidden;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (!contactList?.length) return;
    e.preventDefault();
    if (!open) activeContact = -1;
    const n = matchContacts(contactList, showAll ? "" : toInput.value).slice(0, SHOW_CONTACTS).length;
    if (!n) return;
    activeContact = e.key === "ArrowDown" ? (activeContact + 1) % n : (activeContact <= 0 ? n - 1 : activeContact - 1);
    renderContacts();
  } else if (e.key === "Enter" && open && activeContact >= 0) {
    e.preventDefault(); // Pick, don't send.
    pickContact(shownContacts[activeContact].name);
  } else if (e.key === "Escape" && open) {
    e.preventDefault(); // Close the list, not the popup.
    closeContacts();
  }
});
contactsToggle.addEventListener("mousedown", (e) => e.preventDefault());
contactsToggle.addEventListener("click", () => {
  if (contactsEl.hidden) {
    toInput.focus();
    showAll = true;
    openContacts();
  } else {
    closeContacts();
  }
});

function openCompose(to) {
  composing = true;
  void loadContacts();
  if (typeof to === "string") toInput.value = to;
  $("[data-compose-title]").textContent = typeof to === "string" && to ? `Reply to ${to}` : "New text";
  form.hidden = false;
  waitingEl.hidden = true;
  errEl.hidden = true;
  updateCompose();
  $("[data-new-text]").hidden = true;
  show("compose");
  (toInput.value.trim() ? msgInput : toInput).focus();
}

function closeCompose() {
  composing = false;
  $("[data-new-text]").hidden = false;
  show("ready");
}

function updateCompose() {
  const off = permissions?.send_text === false;
  noteEl.textContent = off
    ? "Sending texts is switched off in tug › Settings › Developer tools › Send texts."
    : sendsNow
      ? "Sends as soon as you click Send. tug asks only if the name could be someone else."
      : "tug asks you to confirm before anything is sent.";
  noteEl.classList.toggle("off", off);
  const n = charCount(msgInput.value);
  countEl.textContent = n > MAX_TEXT_CHARS - 100 ? `${n} / ${MAX_TEXT_CHARS}` : "";
  countEl.classList.toggle("over", n > MAX_TEXT_CHARS);
  sendBtn.disabled = off || sending || composeProblem(toInput.value, msgInput.value) !== null;
  sendBtn.querySelector("span").textContent = sending ? "Sending…" : "Send";
}

function showComposeError(title, body) {
  errEl.replaceChildren(Object.assign(document.createElement("strong"), { textContent: title }), body ? ` ${body}` : "");
  errEl.hidden = false;
}

function showWaiting(to) {
  form.hidden = true;
  waitingEl.hidden = false;
  $("[data-waiting-body]").textContent =
    `tug is showing a card for your text to ${to}. Click Send there. This popup closes when you do; the tug button shows ✓ once it's sent.`;
}

/** Hand the text to the service worker, which waits for tug's card even after this popup closes. */
async function requestSend(to, message) {
  if (globalThis.__tugMock) {
    try {
      const ok = await tug.sendText(to, message);
      mockLast = { state: "done", to, at: Date.now(), result: ok };
      return { ok };
    } catch (e) {
      return { error: { code: e.code ?? "failed", message: e.message } };
    }
  }
  try {
    return (await chrome.runtime.sendMessage({ type: MESSAGE_SEND_TEXT, to, message })) ?? { error: { code: "failed", message: "" } };
  } catch (e) {
    return { error: { code: "failed", message: String(e?.message ?? e) } };
  }
}

toInput.addEventListener("input", updateCompose);
msgInput.addEventListener("input", updateCompose);
msgInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    form.requestSubmit();
  }
});
$("[data-new-text]").addEventListener("click", () => openCompose(""));
$("[data-compose-close]").addEventListener("click", closeCompose);
$("[data-compose-cancel]").addEventListener("click", closeCompose);

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (sending) return;
  const to = toInput.value.trim();
  const message = msgInput.value;
  const problem = composeProblem(to, message);
  if (problem) return showComposeError(problem);
  sending = true;
  errEl.hidden = true;
  updateCompose();
  // tug answers at once when it can't tell who it's to; otherwise its card is up by now. Sending
  // straight away takes a few seconds while the phone sends it, and the card only comes for a
  // name that might be someone else, so wait longer before pointing at tug.
  const waitTimer = setTimeout(() => showWaiting(to), sendsNow ? 6000 : 800);
  const reply = await requestSend(to, message);
  clearTimeout(waitTimer);
  sending = false;
  updateCompose();
  if (reply?.error) {
    await forgetLastText(); // Said here, so not again next time.
    const why = explainSendError(reply.error.code, reply.error.message);
    if (why.screen) return showScreen(why);
    form.hidden = false;
    waitingEl.hidden = true;
    showComposeError(why.title, why.body);
    updateCompose();
    return;
  }
  // tug answered while the popup was still open: say how it went on the main view.
  toInput.value = "";
  msgInput.value = "";
  form.hidden = false;
  waitingEl.hidden = true;
  closeCompose();
  await renderLastText();
});

// The shortcut as the person has set it (chrome://extensions/shortcuts), not the default.
try {
  const fill = (await chrome.commands.getAll()).find((c) => c.name === "fill-code");
  const hint = $("[data-foot-hint]");
  if (fill?.shortcut) {
    hint.replaceChildren(...fill.shortcut.split("+").map((k) => Object.assign(document.createElement("kbd"), { textContent: k })), " fills a code");
  } else {
    hint.textContent = "Right-click a code box to fill it";
  }
} catch {
  // Previewed outside the extension.
}

if (mock === "search") {
  input.value = "dinner";
  input.dispatchEvent(new Event("input"));
}
await renderLastText();
await refresh();
if (mock === "copied") $("[data-copy]").click();
if (mock === "compose") openCompose("Maya Chen");

let tick = 0;
setInterval(() => {
  if (document.hidden || composing) return;
  tick += 1;
  if (tick % 2 === 0) void refresh();
  else if (shownNp) void refresh({ code: false });
}, TICK_MS);
