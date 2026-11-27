// The options page: what the extension does, and a connection check that says exactly which of
// tug's switches are on.

import { tug, TugError } from "../lib/tug.js";
import { explainError, phoneLine } from "../lib/format.js";

const $ = (sel) => document.querySelector(sel);
const ICONS = "../styles/icons.svg";

const mock = new URLSearchParams(location.search).get("mock");
if (mock) {
  const { installMock } = await import("../lib/mock.js");
  installMock(mock);
}

/** The switches in tug › Settings › Developer tools the extension uses, in tug's words. */
const SWITCHES = [
  ["codes", "Verification codes"],
  ["search", "Search texts and notifications"],
  ["media", "Music controls"],
  ["phone_status", "Phone status"],
  ["send_text", "Send texts"],
];

function row(kind, what, why = "") {
  const li = document.createElement("li");
  li.className = `check ${kind}`;
  const badge = document.createElement("span");
  badge.className = "badge";
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "icon");
  const use = document.createElementNS(ns, "use");
  use.setAttribute("href", `${ICONS}#${kind === "ok" ? "check" : kind === "no" ? "x" : "power"}`);
  svg.append(use);
  badge.append(svg);
  li.append(
    badge,
    Object.assign(document.createElement("span"), { className: "what", textContent: what }),
    Object.assign(document.createElement("span"), { className: "why", textContent: why }),
  );
  return li;
}

async function check() {
  const btn = $("[data-check]");
  const list = $("[data-checks]");
  const summary = $("[data-check-summary]");
  // aria-disabled, not disabled: a disabled button drops keyboard focus to the page.
  if (btn.getAttribute("aria-disabled") === "true") return;
  btn.setAttribute("aria-disabled", "true");
  summary.textContent = "Checking…";
  try {
    const s = await tug.status();
    const rows = [row("ok", "tug is running", `Version ${s.version}`)];
    for (const [key, label] of SWITCHES) {
      rows.push(s.permissions?.[key] ? row("ok", label, "On") : row("off", label, "Off in tug"));
    }
    if (s.phone) {
      const p = phoneLine(s.phone);
      rows.push(row(p.state === "connected" ? "ok" : "off", p.label, p.state === "connected" ? `Connected${p.detail ? ` · ${p.detail}` : ""}` : p.detail));
    }
    list.replaceChildren(...rows);
    summary.textContent = "The extension can reach tug.";
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    list.replaceChildren(row("no", why.title, ""));
    summary.textContent = why.body;
  } finally {
    list.hidden = false;
    btn.removeAttribute("aria-disabled");
  }
}

$("[data-check]").addEventListener("click", () => void check());
$("[data-shortcuts]").addEventListener("click", () => {
  void chrome.tabs?.create({ url: "chrome://extensions/shortcuts" });
});

try {
  $("[data-version]").textContent = `Version ${chrome.runtime.getManifest().version}`;
  const fill = (await chrome.commands.getAll()).find((c) => c.name === "fill-code");
  if (fill?.shortcut) $("[data-shortcut]").textContent = fill.shortcut;
  else if (fill) $("[data-shortcut-or]").textContent = "(no keyboard shortcut is set: add one with Change shortcuts below)";
} catch {
  // Previewed outside the extension.
}

if (mock) void check();
