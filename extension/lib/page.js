// Functions the service worker injects into the page (chrome.scripting.executeScript `func`).
// Each must be self-contained: no imports, no outer variables. They run only on a person's
// gesture (the right-click item or the keyboard shortcut), in the tab they're looking at.

/**
 * Put `code` in the page's verification code field. Returns how: "field" (one box), "boxes"
 * (one digit per box), or "none" when there's no field to fill.
 */
export function fillCodeInPage(code) {
  const TEXTISH = new Set(["", "text", "tel", "number", "password", "search"]);
  const HINT = /one.?time|otp|passcode|verif|2fa|mfa|security.?code|auth.?code|sms.?code|\bcode\b|token|\bpin\b/i;

  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) return false;
    const st = getComputedStyle(el);
    return st.visibility !== "hidden" && st.display !== "none" && Number(st.opacity) > 0.05;
  };
  const usable = (el) =>
    el instanceof HTMLInputElement && TEXTISH.has(el.type) && !el.disabled && !el.readOnly && visible(el);

  // The focused element, through open shadow roots (sign-in pages built from web components).
  let active = document.activeElement;
  while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;

  const collect = (root, out) => {
    for (const el of root.querySelectorAll("input, *")) {
      if (el instanceof HTMLInputElement) out.push(el);
      else if (el.shadowRoot) collect(el.shadowRoot, out);
    }
    return out;
  };
  const inputs = collect(document, []).filter(usable);

  // React, Vue and friends watch the value setter and these events, not the attribute.
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  const put = (el, value) => {
    el.focus();
    setter.call(el, value);
    el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, data: value, inputType: "insertText" }));
    el.dispatchEvent(new Event("change", { bubbles: true, composed: true }));
  };

  // One box per digit: maxlength 1 inputs, grouped under the nearest ancestor that holds enough.
  const boxes = inputs.filter((el) => el.maxLength === 1);
  if (boxes.length >= code.length) {
    const start = boxes.includes(active) ? active : boxes[0];
    let group = null;
    for (let node = start.parentElement; node && !group; node = node.parentElement) {
      const inside = boxes.filter((b) => node.contains(b));
      if (inside.length >= code.length) group = inside;
    }
    if (group) {
      group.slice(0, code.length).forEach((el, i) => put(el, code[i]));
      group[Math.min(code.length, group.length) - 1].blur?.();
      return "boxes";
    }
  }

  const describe = (el) =>
    [el.name, el.id, el.placeholder, el.getAttribute("aria-label"), el.labels?.[0]?.textContent]
      .filter(Boolean)
      .join(" ");
  const target =
    (usable(active) ? active : null) ||
    inputs.find((el) => el.autocomplete === "one-time-code") ||
    inputs.find((el) => HINT.test(describe(el)) && el.type !== "password") ||
    null;
  if (!target) return "none";
  put(target, code);
  return "field";
}

/**
 * A small card at the top right of the page saying what happened. Drawn in a closed shadow
 * root so the page's styles can't reach it (nor it the page's), gone after a few seconds.
 * `toast` = { tone: "success" | "info" | "error", title, detail }.
 */
export function showToastInPage(toast) {
  const ID = "tug-extension-toast";
  document.getElementById(ID)?.remove();

  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const c = dark
    ? { bg: "#252320", text: "#faf9f5", soft: "#a09d96", line: "rgba(250,249,245,0.08)" }
    : { bg: "#faf9f5", text: "#141413", soft: "#6c6a64", line: "#e6dfd8" };
  const tone = { success: "#5db872", info: "#cc785c", error: "#c64545" }[toast.tone] || "#cc785c";
  const icon = {
    success: '<path d="M20 6 9 17l-5-5"/>',
    info: '<rect width="8" height="4" x="8" y="2" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
    error: '<circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>',
  }[toast.tone] || "";

  const host = document.createElement("div");
  host.id = ID;
  host.style.cssText = "all:initial;position:fixed;top:16px;right:16px;z-index:2147483647;";
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `
    <style>
      .card { box-sizing: border-box; width: 320px; display: flex; gap: 12px; align-items: flex-start;
        padding: 14px 16px; border-radius: 12px; background: ${c.bg}; color: ${c.text};
        border: 1px solid ${c.line};
        box-shadow: 0 1px 2px rgba(20,20,19,.08), 0 12px 32px -8px rgba(20,20,19,${dark ? ".55" : ".22"});
        font: 14px/1.45 Inter, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
        ${still ? "" : "animation: in 220ms cubic-bezier(.2,.8,.2,1) both;"} }
      .card.out { ${still ? "opacity:0;" : "animation: out 180ms ease-in both;"} }
      @keyframes in { from { opacity: 0; transform: translateY(-6px) scale(.98); } }
      @keyframes out { to { opacity: 0; transform: translateY(-4px); } }
      .icon { flex: none; width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center;
        background: color-mix(in srgb, ${tone} 16%, transparent); color: ${tone}; }
      .icon svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2;
        stroke-linecap: round; stroke-linejoin: round; }
      .body { min-width: 0; flex: 1; }
      .title { font-weight: 500; letter-spacing: -0.005em; }
      .detail { margin-top: 2px; font-size: 13px; color: ${c.soft}; }
      .by { margin-top: 6px; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; color: ${c.soft}; }
      .x { all: unset; cursor: pointer; flex: none; width: 24px; height: 24px; border-radius: 6px;
        display: grid; place-items: center; color: ${c.soft}; }
      .x:hover { background: ${dark ? "#1f1e1b" : "#f5f0e8"}; color: ${c.text}; }
      .x:focus-visible { outline: 2px solid #cc785c; outline-offset: 1px; }
    </style>
    <div class="card" role="status" aria-live="polite">
      <div class="icon"><svg viewBox="0 0 24 24" aria-hidden="true">${icon}</svg></div>
      <div class="body">
        <div class="title"></div>
        <div class="detail"></div>
        <div class="by">tug</div>
      </div>
      <button class="x" aria-label="Dismiss"><svg viewBox="0 0 24 24" width="14" height="14" fill="none"
        stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg></button>
    </div>`;
  // Text from tug goes in as text, never as markup.
  root.querySelector(".title").textContent = toast.title || "";
  const detail = root.querySelector(".detail");
  if (toast.detail) detail.textContent = toast.detail;
  else detail.remove();

  const card = root.querySelector(".card");
  let timer;
  const close = () => {
    clearTimeout(timer);
    card.classList.add("out");
    setTimeout(() => host.remove(), still ? 0 : 180);
  };
  root.querySelector(".x").addEventListener("click", close);
  card.addEventListener("mouseenter", () => clearTimeout(timer));
  card.addEventListener("mouseleave", () => (timer = setTimeout(close, 2500)));
  (document.body || document.documentElement).appendChild(host);
  timer = setTimeout(close, toast.tone === "error" ? 6000 : 4000);
}
