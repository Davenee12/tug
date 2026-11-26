// "Fill code from iPhone": the right-click item on a text field and the keyboard shortcut.
// Both are gestures on the tab in front, so activeTab is all the page access the extension has:
// it never reads pages on its own.

import { tug, TugError } from "./lib/tug.js";
import { explainError, groupCode, codeSource } from "./lib/format.js";
import { fillCodeInPage, showToastInPage } from "./lib/page.js";

const MENU_ID = "fill-code";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: MENU_ID,
    title: "Fill code from iPhone",
    contexts: ["editable"],
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
  try {
    const [result] = await chrome.scripting.executeScript({
      target: { tabId, frameIds: [frameId] },
      func: fillCodeInPage,
      args: [code.code],
    });
    how = result?.result ?? "none";
  } catch {
    // The field is in a frame from another site, which activeTab doesn't reach: copy instead.
  }

  if (how !== "none") {
    await toast(tabId, { tone: "success", title: `Filled ${shown}`, detail: `From ${from}` });
    return;
  }
  // No field to fill: put it on the clipboard through tug (kept out of clipboard history).
  try {
    await tug.latestCode(true);
    await toast(tabId, { tone: "info", title: `Copied ${shown}`, detail: "No code field here, so paste it in." });
  } catch (e) {
    const why = explainError(e instanceof TugError ? e.code : "failed", e.message);
    await toast(tabId, { tone: "error", title: why.title, detail: why.body });
  }
}
