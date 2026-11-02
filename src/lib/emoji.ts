// Emoji shortcuts in the message boxes: ":)" becomes 🙂 and ":fire:" becomes 🔥 as you type, the
// way phones and chat apps do. Pure: the boxes (lib/emojiTyping) pass the text and the caret and
// put back what this returns.
//
// When:
// - An emoticon (":)", "<3", ":D") turns into its emoji once it's finished: a space, a new line or
//   . , ! ? typed after it, or Send with it at the end. Not while it's being typed, so ":D" isn't
//   caught as ":" then "D", and "</3" isn't caught at "<".
// - A named one (":fire:") turns as soon as its closing colon is typed.
// - Only standing on its own: at the start or after a space. So "http://", "4:3", "a:)" and
//   "note:fire:" stay as typed.
// - Backspace straight after a change puts back exactly what was typed (lib/emojiTyping keeps the
//   `Undo`), for the time ":P" really was meant.

/** Emoticons and their emoji. Only ones that don't turn up in ordinary writing ("B)" in "plan B)"
 * and "8)" in "(1-8)" do, so they're left out). */
export const EMOTICONS: Readonly<Record<string, string>> = {
  ":)": "🙂",
  ":-)": "🙂",
  "(:": "🙂",
  ":D": "😄",
  ":-D": "😄",
  "XD": "😆",
  "xD": "😆",
  ";)": "😉",
  ";-)": "😉",
  ":(": "🙁",
  ":-(": "🙁",
  ":'(": "😢",
  ":P": "😛",
  ":-P": "😛",
  ":p": "😛",
  ":-p": "😛",
  ";P": "😜",
  ";p": "😜",
  ":O": "😮",
  ":-O": "😮",
  ":o": "😮",
  ":|": "😐",
  ":-|": "😐",
  ":/": "😕",
  ":-/": "😕",
  ":*": "😘",
  ":-*": "😘",
  "^_^": "😊",
  "^^": "😊",
  "-_-": "😑",
  "<3": "❤️",
  "</3": "💔",
};

/** Named emoji, typed as ":name:". */
export const SHORTCODES: Readonly<Record<string, string>> = {
  smile: "😄",
  grin: "😁",
  laughing: "😆",
  joy: "😂",
  rofl: "🤣",
  sweat_smile: "😅",
  wink: "😉",
  blush: "😊",
  heart_eyes: "😍",
  kiss: "😘",
  yum: "😋",
  zany: "🤪",
  upside_down: "🙃",
  thinking: "🤔",
  neutral: "😐",
  eyeroll: "🙄",
  smirk: "😏",
  sunglasses: "😎",
  hug: "🤗",
  shush: "🤫",
  sleepy: "😪",
  sleeping: "😴",
  cry: "😢",
  sob: "😭",
  sweat: "😓",
  angry: "😠",
  rage: "😡",
  scream: "😱",
  flushed: "😳",
  party: "🥳",
  skull: "💀",
  ghost: "👻",
  poop: "💩",
  eyes: "👀",
  shrug: "🤷",
  facepalm: "🤦",
  pray: "🙏",
  clap: "👏",
  wave: "👋",
  ok: "👌",
  thumbsup: "👍",
  "+1": "👍",
  thumbsdown: "👎",
  "-1": "👎",
  raised_hands: "🙌",
  muscle: "💪",
  heart: "❤️",
  broken_heart: "💔",
  fire: "🔥",
  "100": "💯",
  sparkles: "✨",
  star: "⭐",
  tada: "🎉",
  gift: "🎁",
  cake: "🎂",
  coffee: "☕",
  pizza: "🍕",
  beer: "🍺",
  wine: "🍷",
  check: "✅",
  x: "❌",
  warning: "⚠️",
  rocket: "🚀",
  money: "💰",
  sun: "☀️",
  rain: "🌧️",
  snow: "❄️",
  rainbow: "🌈",
  dog: "🐶",
  cat: "🐱",
  phone: "📱",
  car: "🚗",
  boat: "🚤",
};

/** Typed after an emoticon, these say it's finished (and stay in the text after the emoji). */
const ENDS = /[\s.,!?]/;

/** What a change did, so Backspace straight after it can put the typed text back. */
export interface Undo {
  /** The text and caret as typed. */
  before: string;
  beforeCaret: number;
  /** The text and caret just after the change. */
  after: string;
  afterCaret: number;
}

export interface Change {
  text: string;
  caret: number;
  undo: Undo;
}

/** The emoji for one complete token (an emoticon or ":name:"), if it's one. */
export function emojiFor(token: string): string | null {
  if (Object.hasOwn(EMOTICONS, token)) return EMOTICONS[token];
  const named = /^:([a-z0-9_+-]+):$/.exec(token);
  return named && Object.hasOwn(SHORTCODES, named[1]) ? SHORTCODES[named[1]] : null;
}

/** The token that ends at `end` in `text`, standing on its own (at the start or after a space). */
function tokenBefore(text: string, end: number): { start: number; token: string } | null {
  const m = /(?:^|\s)(\S+)$/.exec(text.slice(0, end));
  return m ? { start: end - m[1].length, token: m[1] } : null;
}

function change(text: string, caret: number, start: number, end: number, emoji: string): Change {
  const after = text.slice(0, start) + emoji + text.slice(end);
  const afterCaret = caret - (end - start) + emoji.length;
  return { text: after, caret: afterCaret, undo: { before: text, beforeCaret: caret, after, afterCaret } };
}

/**
 * Just after a character was typed (it sits before `caret`): the change to make, if that finished
 * an emoticon or a ":name:". Null leaves the text as it is.
 */
export function afterTyping(text: string, caret: number): Change | null {
  if (caret < 1 || caret > text.length) return null;
  const typed = text[caret - 1];
  if (typed === ":") {
    const t = tokenBefore(text, caret);
    if (t && /^:[a-z0-9_+-]+:$/.test(t.token)) {
      const emoji = emojiFor(t.token);
      if (emoji) return change(text, caret, t.start, caret, emoji);
    }
  }
  if (ENDS.test(typed)) {
    const t = tokenBefore(text, caret - 1);
    const emoji = t && Object.hasOwn(EMOTICONS, t.token) ? EMOTICONS[t.token] : null;
    if (t && emoji) return change(text, caret, t.start, caret - 1, emoji);
  }
  return null;
}

/** On Send: an emoticon at the very end has nothing typed after it, so it's finished here. */
export function finishForSend(text: string): string {
  const t = tokenBefore(text, text.length);
  return t && Object.hasOwn(EMOTICONS, t.token) ? text.slice(0, t.start) + EMOTICONS[t.token] : text;
}

/** Backspace now: the typed text back, if the box still holds exactly what the last change left. */
export function undoOnBackspace(undo: Undo | null, text: string, caret: number): { text: string; caret: number } | null {
  if (!undo || text !== undo.after || caret !== undo.afterCaret) return null;
  return { text: undo.before, caret: undo.beforeCaret };
}
