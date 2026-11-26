// The toolbar popup: the phone, the newest verification code, search, and now playing. It asks
// tug when it opens and every few seconds while it stays open, and keeps nothing afterwards.

import { tug, TugError } from "../lib/tug.js";
import {
  ago,
  codeAge,
  explainError,
  groupCode,
  initial,
  isOldCode,
  nowPlaying,
  phoneLine,
  phoneNotice,
  shortTime,
} from "../lib/format.js";

const $ = (sel) => document.querySelector(sel);
const REFRESH_MS = 5000;
const ICONS = "../styles/icons.svg";

const mock = new URLSearchParams(location.search).get("mock");
if (mock) {
  const { installMock } = await import("../lib/mock.js");
  installMock(mock);
}

let lastCode = null;

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
  actions.replaceChildren();
  if (why.screen === "install") {
    const a = Object.assign(document.createElement("a"), {
      className: "btn btn-primary",
      href: "https://davenee12.github.io/tug/",
      target: "_blank",
      textContent: "Get tug for Windows",
    });
    actions.append(a);
  } else {
    const retry = Object.assign(document.createElement("button"), { className: "btn btn-secondary", textContent: "Try again" });
    retry.prepend(icon("refresh-cw"));
    retry.addEventListener("click", () => void refresh());
    actions.append(retry);
  }
  $("[data-pill]").hidden = true;
  show("screen");
}

function renderPhone(phone, permissions) {
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

  const np = phone && permissions?.media !== false ? nowPlaying(phone) : null;
  const strip = $("[data-np]");
  strip.hidden = !np;
  if (np) {
    $("[data-np-title]").textContent = np.title;
    $("[data-np-sub]").textContent = np.subtitle;
    $("[data-np-icon]").setAttribute("href", `${ICONS}#${np.playing ? "pause" : "play"}`);
  }
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
  const btn = $("[data-copy]");
  btn.classList.remove("done");
  btn.querySelector("use").setAttribute("href", `${ICONS}#copy`);
  btn.querySelector("span").textContent = "Copy";
}

async function refresh() {
  let status;
  try {
    status = await tug.status();
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    if (why.screen) return showScreen(why);
    status = null;
  }
  renderPhone(status?.phone ?? null, status?.permissions);
  try {
    renderCode(await tug.latestCode(false));
  } catch (e) {
    const err = e instanceof TugError ? e : new TugError("failed", e.message);
    const why = explainError(err.code);
    if (why.screen) return showScreen(why);
    renderCode(null, err);
  }
  show("ready");
}

$("[data-copy]").addEventListener("click", async () => {
  const btn = $("[data-copy]");
  try {
    // tug puts it on the clipboard itself, kept out of clipboard history and sync.
    await tug.latestCode(true);
    btn.classList.add("done");
    btn.querySelector("use").setAttribute("href", `${ICONS}#check`);
    btn.querySelector("span").textContent = "Copied";
    setTimeout(resetCopy, 2400);
  } catch (e) {
    const why = explainError(e.code, e.message);
    $("[data-code-from]").textContent = why.title;
    $("[data-code-app]").textContent = "";
  }
});

for (const btn of document.querySelectorAll("[data-media]")) {
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    try {
      await tug.media(btn.dataset.media);
      setTimeout(() => void refresh(), 400);
    } catch {
      // The strip disappears on the next refresh if the phone went away.
    } finally {
      btn.disabled = false;
    }
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
      const time = Object.assign(document.createElement("span"), { className: "hit-time", textContent: shortTime(h.time) });
      const text = document.createElement("span");
      text.className = "hit-text";
      text.append(highlighted(h.text, query));
      li.append(avatar, who, time, text);
      return li;
    }),
  );
  list.hidden = hits.length === 0;
  note.hidden = hits.length !== 0;
  note.textContent = hits.length ? "" : `Nothing matches “${query}”.`;
}

input.addEventListener("input", () => {
  clearTimeout(searchTimer);
  const query = input.value.trim();
  if (query.length < 2) {
    list.hidden = true;
    note.hidden = true;
    return;
  }
  searchTimer = setTimeout(async () => {
    const seq = ++searchSeq;
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

// "/" jumps to search, as in most apps with one search box.
document.addEventListener("keydown", (e) => {
  if (e.key === "/" && document.activeElement !== input && !e.ctrlKey && !e.metaKey && !e.altKey) {
    e.preventDefault();
    input.focus();
  }
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
await refresh();
if (mock === "copied") $("[data-copy]").click();
setInterval(() => {
  if (!document.hidden) void refresh();
}, REFRESH_MS);
