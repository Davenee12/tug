// "Fill code from iPhone": the right-click item on a text field and the keyboard shortcut.
// Both are gestures on the tab in front, so activeTab is all the page access the extension has:
// it never reads pages on its own.
//
// Texts from the popup are sent here too: the popup closes as soon as the person clicks into tug
// to answer its card, so the wait can't live there (see sendText below).

import { tug, TugError } from "./lib/tug.js";
import { explainError, groupCode, codeSource } from "./lib/format.js";
import { fillCodeInPage, showToastInPage } from "./lib/page.js";
import { LAST_TEXT, MESSAGE_SEND_TEXT } from "./lib/texts.js";

const MENU_ID = "fill-code";

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg?.type !== MESSAGE_SEND_TEXT || sender.id !== chrome.runtime.id) return false;
  void sendText(String(msg.to ?? ""), String(msg.message ?? "")).then(sendResponse);
  return true; // Answered later, if the popup is still open by then.
});

/**
 * Send a text through tug and keep what happened for the popup: who it was to and the outcome,
 * never the message, in storage.session (memory only, gone when the browser closes). The
 * toolbar badge says how it went while the popup is closed: … waiting, ✓ sent, ! anything else.
 */
async function sendText(to, message) {
  await remember({ state: "waiting", to, at: Date.now() });
  await badge("…", "#64625c");
  try {
    const result = await tug.sendText(to, message);
    await remember({ state: "done", to, at: Date.now(), result });
    await badge(result?.outcome === "sent" ? "✓" : "!", result?.outcome === "sent" ? "#2a6e3c" : "#a9583e");
    return { ok: result };
  } catch (e) {
    const error = { code: e instanceof TugError ? e.code : "failed", message: String(e?.message ?? "") };
    await remember({ state: "error", to, at: Date.now(), error });
    await badge("!", "#a33a3a");
    return { error };
  }
}

async function remember(value) {
  try {
    await chrome.storage.session.set({ [LAST_TEXT]: value });
  } catch {
    // Nowhere to keep it: the badge still says how it went.
  }
}

async function badge(text, color) {
  try {
    await chrome.action.setBadgeBackgroundColor({ color });
    await chrome.action.setBadgeText({ text });
  } catch {
    // Not fatal.
  }
}

chrome.runtime.onInstalled.addListener(() => {
  // Also runs on updates, when the item is already there.
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: "Fill code from iPhone",
      contexts: ["editable"],
    });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === MENU_ID && tab?.id != null) void fillCode(tab.id, info.frameId ?? 0);
});

chrome.commands.onCommand.addListener((command, tab) => {
  if (command === "fill-code" && tab?.id != null) void fillCode(tab.id, 0);
});

async function toast(tabId, t) {
  try {
    await chrome.scripting.executeScript({ target: { tabId }, func: showToastInPage, args: [t] });
  } catch {
    // A page the extension may not touch (the Web Store, chrome:// pages): nothing to show on.
  }
}

async function fillCode(tabId, frameId) {
  let code;
  try {
    code = await tug.latestCode(false);
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    await toast(tabId, { tone: "error", title: why.title, detail: why.body });
    return;
  }

  const shown = groupCode(code.code);
  const from = codeSource(code.from, code.app);
  let how = "none";
  let reached = true;
  try {
    const [result] = await chrome.scripting.executeScript({
      target: { tabId, frameIds: [frameId] },
      func: fillCodeInPage,
      args: [code.code],
    });
    how = result?.result ?? "none";
  } catch {
    // The field is in a frame from another site, which activeTab doesn't reach: copy instead.
    reached = false;
  }

  if (how !== "none") {
    await toast(tabId, { tone: "success", title: `Filled ${shown}`, detail: `From ${from}` });
    return;
  }
  // No field to fill: put it on the clipboard through tug (kept out of clipboard history).
  try {
    const copied = await tug.latestCode(true);
    const now = groupCode(copied.code);
    if (!copied.copied) {
      // tug couldn't get the clipboard (another app holding it): say so, never "Copied".
      await toast(tabId, { tone: "error", title: `Code ${now}`, detail: "Couldn't put it on the clipboard. Type it in." });
      return;
    }
    const why = reached ? "No code field here, so paste it in." : "Couldn't reach the code box here, so paste it in.";
    await toast(tabId, { tone: "info", title: `Copied ${now}`, detail: why });
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    await toast(tabId, { tone: "error", title: why.title, detail: why.body });
  }
}
