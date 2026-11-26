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
  await browser.close();
  process.exit(failed ? 1 : 0);
})();
