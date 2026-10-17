// Suggested replies: up to three one-tap answers to an incoming text ("On my way", "Yes", "👍"),
// picked on this PC from what the text says. No network, no AI service: a few plain rules, in
// order, the first that matches wins. Shown as buttons on a text's Windows pop-up and as chips
// above the composer in Messages; a tap sends through the same path as a typed reply.
//
// Nothing is suggested for automated texts (one-time codes, short codes, "Reply STOP"): answering
// a bank's code or a delivery robot with "👍" is never what anyone meant to do.

import type { SmsMessage } from "../types/protocol";
import { findCode } from "./codes";
import { MESSAGES_APP, type Conversation, type ConversationItem } from "./format";

/** At most this many suggestions, so they fit beside Send and Mark read on a pop-up (Windows allows 5 buttons). */
export const MAX_REPLIES = 3;
/** Longest suggestion (characters). Toast buttons are narrow, and a reply this long is better typed. Mirrors `REPLY_TEXT_MAX` in `toast/xml.rs`. */
export const REPLY_MAX_CHARS = 40;
/** What a text gets when no rule below matches. Editable in Settings › Notifications. */
export const DEFAULT_REPLIES: readonly string[] = ["👍", "OK", "Talk soon"];

export interface ReplyRule {
  /** For tests and Settings' explanation; never logged with a message. */
  name: "call" | "eta" | "question" | "thanks";
  test: RegExp;
  replies: readonly string[];
}

/**
 * The rules, most specific first: "Can you call me?" is a call request before it's a question, and
 * "Where are you?" is about where you are before it's a question.
 */
export const RULES: readonly ReplyRule[] = [
  {
    name: "call",
    test: /\b(call|ring|phone) me\b|\bgive me a (call|ring)\b|\bcan you call\b/i,
    replies: ["Calling you now", "Call you later"],
  },
  {
    name: "eta",
    test: /\bwhere (are|r) (you|u)\b|\beta\b|\bhow long\b|\bomw\b|\bon (your|ur) way\b|\bhow far\b|\bwhen will (you|u) (be|get)\b/i,
    replies: ["On my way", "5 minutes", "Running late"],
  },
  {
    name: "question",
    test: /\?|\b(are|can|did|do|will|could|would|have) (you|u)\b|\bwhat time\b/i,
    replies: ["Yes", "No", "Let me check"],
  },
  {
    name: "thanks",
    test: /\b(thanks|thank you|thank u|thanx|thx|ty|tysm|cheers|much appreciated|appreciate it)\b/i,
    replies: ["You're welcome", "👍"],
  },
];

/** Wording automated senders use: opt-outs and no-reply notices. */
const AUTOMATED = /\b(reply|text|txt|send)\s+(stop|unstop|help)\b|\bunsubscribe\b|\bopt[- ]?out\b|\b(do not|don't|please don't) reply\b|\bno-?reply\b|\bmsg\s*&\s*data rates\b/i;

/**
 * A short code (a 3–6 digit sender like "72975"), which is a business or a robot, never a person
 * waiting for "On my way". Emails and full numbers aren't.
 */
export function isShortCode(address: string | null | undefined): boolean {
  if (!address || address.includes("@")) return false;
  const t = address.trim();
  if (!/^\+?[\d\s().-]+$/.test(t)) return false;
  const digits = t.replace(/\D/g, "").length;
  return digits > 0 && digits <= 6;
}

/**
 * Whether a text reads as automated: it carries a one-time code (tug's own code detection, the same
 * one behind Copy code), or an opt-out / no-reply line.
 */
export function looksAutomated(text: string): boolean {
  return findCode(text) !== null || AUTOMATED.test(text);
}

/**
 * A suggestion list made safe to show and send: trimmed, no control characters, no blanks, no
 * repeats (ignoring case), each at most {@link REPLY_MAX_CHARS} characters, at most
 * {@link MAX_REPLIES}. Used for the defaults people type in Settings too.
 */
export function cleanReplies(list: readonly unknown[] | null | undefined): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of list ?? []) {
    if (typeof raw !== "string") continue;
    // Control characters become spaces: a toast's XML would refuse them, and a reply never needs one.
    const text = raw.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").replace(/\s+/g, " ").trim();
    if (!text || Array.from(text).length > REPLY_MAX_CHARS) continue;
    const key = text.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(text);
    if (out.length === MAX_REPLIES) break;
  }
  return out;
}

/**
 * The saved default list (`ui.smartReplyDefaults`): never saved means {@link DEFAULT_REPLIES}; a
 * saved list is cleaned (so an emptied list stays empty: no suggestions when no rule matches);
 * anything unreadable falls back to the defaults.
 */
export function parseReplyDefaults(raw: string | null | undefined): string[] {
  if (raw == null || raw === "") return [...DEFAULT_REPLIES];
  try {
    const list: unknown = JSON.parse(raw);
    return Array.isArray(list) ? cleanReplies(list) : [...DEFAULT_REPLIES];
  } catch {
    return [...DEFAULT_REPLIES];
  }
}

export interface ReplyOptions {
  /** Settings › Notifications › Suggested replies (on by default). */
  enabled: boolean;
  /** The list for texts no rule matches (Settings); {@link DEFAULT_REPLIES} unless changed. */
  defaults: readonly string[];
  /** Who sent it (number or email), to leave short codes out. */
  sender?: string | null;
}

/**
 * Up to three replies for an incoming text, or none: switched off, nothing to read (a photo, or a
 * notification without a preview), or an automated text.
 */
export function suggestReplies(text: string | null | undefined, opts: ReplyOptions): string[] {
  if (!opts.enabled) return [];
  const body = (text ?? "").trim();
  if (!body || isShortCode(opts.sender) || looksAutomated(body)) return [];
  const rule = RULES.find((r) => r.test.test(body));
  return cleanReplies(rule ? rule.replies : opts.defaults);
}

/** Which rule a text falls under ("default" when none), for tests. */
export function ruleFor(text: string): ReplyRule["name"] | "default" {
  return RULES.find((r) => r.test.test(text.trim()))?.name ?? "default";
}

/** An incoming item of a conversation: its text and who sent it. Null for what you sent. */
function incoming(item: ConversationItem, fallbackSender: string | null): { text: string; sender: string | null } | null {
  if (item.kind === "message") {
    const m: SmsMessage = item.m;
    return m.direction === "in" ? { text: m.body, sender: m.address } : null;
  }
  return { text: item.n.message || item.n.subtitle, sender: fallbackSender };
}

/**
 * The chips above the composer in Messages: replies to the conversation's newest item, only while
 * that's a text they sent (once you've answered, or a send of yours is the newest, there are none).
 * Only Messages conversations, the ones tug can reply to.
 */
export function conversationReplies(c: Pick<Conversation, "appId" | "items" | "address"> | null, opts: Omit<ReplyOptions, "sender">): string[] {
  if (!c || c.appId !== MESSAGES_APP || !opts.enabled) return [];
  const last = c.items[c.items.length - 1];
  if (!last) return [];
  const from = incoming(last, c.address);
  return from ? suggestReplies(from.text, { ...opts, sender: from.sender }) : [];
}
