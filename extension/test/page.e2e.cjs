// The in-page "Fill code" and toast, run in a real Chromium against sign-in pages shaped like
// real ones. Needs Playwright with a browser (not a project dependency):
//   node extension/test/page.e2e.cjs
const { chromium } = require("playwright");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");

const source = fs.readFileSync(path.join(__dirname, "../lib/page.js"), "utf8").replace(/^export /gm, "");

const CASES = [
  {
    name: "a one-time-code field, not focused",
    html: `<input type="email" name="email"><input autocomplete="one-time-code" id="otp">`,
    how: "field",
    check: () => document.getElementById("otp").value === "482913" && document.querySelector("[name=email]").value === "",
  },
  {
    name: "the focused field wins",
    html: `<input id="a" autocomplete="one-time-code"><input id="b">`,
    focus: "#b",
    how: "field",
    check: () => document.getElementById("b").value === "482913" && document.getElementById("a").value === "",
  },
  {
    name: "a field named for codes, next to an email field",
    html: `<label>Email <input type="email" id="e"></label><label>Verification code <input id="c" inputmode="numeric"></label>`,
    how: "field",
    check: () => document.getElementById("c").value === "482913" && document.getElementById("e").value === "",
  },
  {
    name: "one box per digit, with a framework listening for input events",
    html: `<input type="email"><div class="otp">${'<input maxlength="1" inputmode="numeric">'.repeat(6)}</div>
      <script>window.seen=[];document.querySelectorAll('.otp input').forEach(i=>i.addEventListener('input',e=>seen.push(e.target.value)))</script>`,
    how: "boxes",
    check: () =>
      [...document.querySelectorAll(".otp input")].map((i) => i.value).join("") === "482913" && window.seen.join("") === "482913",
  },
  {
    name: "inside a web component's shadow root",
    html: `<sign-in></sign-in><script>customElements.define('sign-in',class extends HTMLElement{connectedCallback(){this.attachShadow({mode:'open'}).innerHTML='<input autocomplete="one-time-code">'}})</script>`,
    how: "field",
    check: () => document.querySelector("sign-in").shadowRoot.querySelector("input").value === "482913",
  },
  {
    name: "a hidden code field is left alone",
    html: `<input autocomplete="one-time-code" style="display:none"><input type="search" placeholder="Search">`,
    how: "none",
    check: () => true,
  },
  {
    name: "no code field: nothing is filled",
    html: `<input type="search" placeholder="Search the docs"><textarea></textarea>`,
    how: "none",
    check: () => document.querySelector("input").value === "",
  },
  {
    name: "checkout fields that mention a code are left alone",
    html: `<label>Promo code <input id="p"></label><label>Security code <input id="cvc"></label><label>ZIP code <input id="z"></label>`,
    how: "none",
    check: () => ["p", "cvc", "z"].every((id) => document.getElementById(id).value === ""),
  },
  {
    name: "focus in a frame (a bank's challenge): nothing up here is filled",
    html: `<label>Promo code <input id="p"></label><input name="otp_code" id="o"><iframe id="f" srcdoc="<input>"></iframe>`,
    focus: "#f",
    how: "none",
    check: () => document.getElementById("o").value === "" && document.getElementById("p").value === "",
  },
  {
    name: "one box per digit, each in its own shadow root",
    html: `<div class="otp">${"<otp-box></otp-box>".repeat(6)}</div><script>customElements.define('otp-box',class extends HTMLElement{connectedCallback(){this.attachShadow({mode:'open'}).innerHTML='<input maxlength="1">'}})</script>`,
    how: "boxes",
    check: () => [...document.querySelectorAll("otp-box")].map((b) => b.shadowRoot.querySelector("input").value).join("") === "482913",
  },
  {
    name: "one box per digit, rebuilt from state on each render (React-style)",
    html: `<div class="otp">${'<input maxlength="1">'.repeat(6)}</div>
      <script>
        let state = Array(6).fill("");
        const boxes = [...document.querySelectorAll(".otp input")];
        const render = () => boxes.forEach((b, i) => {
          b.value = state[i];
          const seen = state; // What this render saw, as a component's closure would.
          b.oninput = (e) => { const next = [...seen]; next[i] = e.target.value; queueMicrotask(() => { state = next; render(); }); };
        });
        render();
      </script>`,
    how: "boxes",
    check: () => [...document.querySelectorAll(".otp input")].map((i) => i.value).join("") === "482913",
  },
];

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  let failed = 0;
  for (const c of CASES) {
    await page.setContent(`<!doctype html><body>${c.html}</body>`);
    if (c.focus) await page.focus(c.focus);
    await page.addScriptTag({ content: source });
    const how = await page.evaluate(() => fillCodeInPage("482913"));
    const ok = await page.evaluate(c.check);
    const pass = how === c.how && ok;
    if (!pass) failed++;
    console.log(`${pass ? "ok  " : "FAIL"} ${c.name}${pass ? "" : ` (how=${how}, check=${ok})`}`);
  }

  // The toast: tug's text goes in as text, never markup, and the page's CSS can't restyle it.
  await page.setContent(`<!doctype html><style>div{color:red!important;font-size:40px!important}</style><body></body>`);
  await page.addScriptTag({ content: source });
  await page.evaluate(() =>
    showToastInPage({ tone: "success", title: 'Filled <img src=x onerror="window.pwned=1">', detail: "From Chase" }),
  );
  await page.waitForTimeout(50);
  const toast = await page.evaluate(() => ({
    present: !!document.getElementById("tug-extension-toast"),
    pwned: !!window.pwned,
    images: document.querySelectorAll("img").length,
  }));
  try {
    assert.deepEqual(toast, { present: true, pwned: false, images: 0 });
    console.log("ok   the toast shows tug's text as text");
  } catch (e) {
    failed++;
    console.log("FAIL the toast", toast);
  }

  // Above an open modal dialog, which would otherwise cover it and make it inert.
  await page.setContent(`<!doctype html><body><dialog id="d">Sign in</dialog><script>d.showModal()</script></body>`);
  await page.addScriptTag({ content: source });
  await page.evaluate(() => showToastInPage({ tone: "success", title: "Filled 482 913" }));
  const onTop = await page.evaluate(() => document.getElementById("tug-extension-toast").matches(":popover-open"));
  console.log(`${onTop ? "ok  " : "FAIL"} the toast shows above a modal dialog`);
  if (!onTop) failed++;
  await browser.close();
  process.exit(failed ? 1 : 0);
})();
