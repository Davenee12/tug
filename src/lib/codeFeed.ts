// Verification codes that arrive as texts, surfaced in the Feed. Some codes come in over MAP
// (Messages) without ever raising an ANCS notification — the iPhone shows no banner, so nothing
// lands in the Feed and the text waits in Messages (often under "Unknown senders"). This derives
// a Feed row straight from the texts so the code is one click away, de-duped against any ANCS
// notification that did carry it (that row already has Copy code).
//
// Pure module: no Tauri, no DOM, no clock of its own (the caller passes `now`), so it's all
// unit-tested. Keys match `groupConversations`, so opening a row lands on the right conversation.

import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";
import { findCode, type FoundCode } from "./codes";
import { MESSAGES_APP, cleanName, formatAddress, isConversation, threadKey } from "./format";
import { isAddressLike } from "./address";

/** How recent a text must be to show its code in the Feed. */
const DEFAULT_MAX_AGE_MS = 15 * 60 * 1000;
/** A text and an ANCS notification are the same message if their codes match within this window. */
const SAME_MESSAGE_MS = 10 * 60 * 1000;
/** Only a text received this recently is a live arrival worth a pop-up (not startup backlog). */
const DEFAULT_LIVE_MS = 2 * 60 * 1000;

/** One code waiting in the Feed, derived from one or more texts carrying the same code. */
export interface CodeEntry {
  /** Stable Feed-row key (conversation + code). */
  key: string;
  /** Opens this conversation in Messages; matches `groupConversations`/`threadKey`. */
  conversationKey: string;
  /** Who it's from, named as the conversation is (contact name, else the number/short code). */
  sender: string;
  /** The detected code (digits to copy, and as written for display). */
  code: FoundCode;
  /** The text itself, for the row preview. */
  body: string;
  /** The newest backing text's receive time (unix ms), for ordering against the notification feed. */
  at: number;
  /** Every text behind this row — duplicate codes (three identical OTPs) collapse into one; Clear hides them all. */
  messageIds: number[];
}

export interface CodeFeedOptions {
  /** The current time; defaults to now. Passed in so recency is testable and reactive. */
  now?: number;
  /** How far back a text still counts (default 15 min). */
  maxAgeMs?: number;
  /** Window for matching a text to an ANCS notification carrying the same code (default 10 min). */
  windowMs?: number;
  /** Message ids the user has cleared from the Feed (hidden, not deleted). */
  cleared?: Iterable<number>;
  /** Contacts, to name a known sender the same way the conversation list does. */
  contacts?: Contact[];
}

/** How the conversation names a text's sender: contact name, else the number/short code formatted. */
function senderName(m: SmsMessage, nameFor: Map<string, string>): string {
  const known = m.contactName ?? nameFor.get(m.address);
  return known && !isAddressLike(known) ? cleanName(known) : formatAddress(m.address);
}

interface NoteCode {
  conversationKey: string;
  code: string;
  at: number;
}

/** The codes carried by Messages notifications (any state), to de-dupe texts against. */
function notificationCodes(notifications: PhoneNotification[]): NoteCode[] {
  const out: NoteCode[] = [];
  for (const n of notifications) {
    if (n.appId !== MESSAGES_APP || !isConversation(n)) continue;
    const found = findCode(n.message || n.subtitle, n.title);
    if (found) out.push({ conversationKey: threadKey(n), code: found.code, at: n.receivedAt });
  }
  return out;
}

/** Is this code, from this sender, already on an ANCS notification (which has its own Copy code)? */
function coveredByNotification(index: NoteCode[], conversationKey: string, code: string, at: number, windowMs: number): boolean {
  return index.some((x) => x.code === code && x.conversationKey === conversationKey && Math.abs(x.at - at) < windowMs);
}

/**
 * The code-carrying texts to show in the Feed: incoming, recent, not cleared, and not already
 * shown by an ANCS notification. Texts with the same code from the same sender collapse into one
 * row (the newest), so three identical OTP texts are a single entry. Newest first.
 */
export function codeEntries(messages: SmsMessage[], notifications: PhoneNotification[], opts: CodeFeedOptions = {}): CodeEntry[] {
  const now = opts.now ?? Date.now();
  const since = now - (opts.maxAgeMs ?? DEFAULT_MAX_AGE_MS);
  const windowMs = opts.windowMs ?? SAME_MESSAGE_MS;
  const cleared = new Set(opts.cleared ?? []);
  const nameFor = new Map((opts.contacts ?? []).map((c) => [c.address, c.name]));
  const index = notificationCodes(notifications);

  const groups = new Map<string, CodeEntry>();
  for (const m of messages) {
    if (m.direction !== "in" || m.receivedAt < since || cleared.has(m.id)) continue;
    const found = findCode(m.body, m.address);
    if (!found) continue;
    const conversationKey = threadKey({ appId: MESSAGES_APP, title: senderName(m, nameFor) });
    if (coveredByNotification(index, conversationKey, found.code, m.receivedAt, windowMs)) continue;
    const groupKey = `${conversationKey}\u0000${found.code}`;
    const existing = groups.get(groupKey);
    if (existing) {
      existing.messageIds.push(m.id);
      if (m.receivedAt > existing.at) {
        existing.at = m.receivedAt;
        existing.body = m.body;
        existing.code = found;
      }
    } else {
      groups.set(groupKey, {
        key: `code\u0000${groupKey}`,
        conversationKey,
        sender: senderName(m, nameFor),
        code: found,
        body: m.body,
        at: m.receivedAt,
        messageIds: [m.id],
      });
    }
  }
  return [...groups.values()].sort((a, b) => b.at - a.at);
}

export interface LatestCode {
  code: string;
  /** The notification(s) it came on, to clear once copied; empty for a code that arrived as a text. */
  from: PhoneNotification[];
}

export interface NewestCodeOptions {
  now?: number;
  maxAgeMs?: number;
}

/**
 * The newest one-time code waiting, from a notification or a text, within `maxAgeMs` (default
 * 10 min). Powers Ctrl+Shift+C and the Ctrl+K "copy code" action; newest wins across both sources.
 */
export function newestCode(
  notifications: PhoneNotification[],
  messages: SmsMessage[],
  opts: NewestCodeOptions = {},
): LatestCode | null {
  const now = opts.now ?? Date.now();
  const since = now - (opts.maxAgeMs ?? SAME_MESSAGE_MS);
  let best: { code: string; at: number; from: PhoneNotification[] } | null = null;
  for (const n of notifications) {
    if (n.receivedAt < since) continue;
    const found = findCode(n.message || n.subtitle, n.title);
    if (found && (!best || n.receivedAt > best.at)) best = { code: found.code, at: n.receivedAt, from: [n] };
  }
  for (const m of messages) {
    if (m.direction !== "in" || m.receivedAt < since) continue;
    const found = findCode(m.body, m.address);
    if (found && (!best || m.receivedAt > best.at)) best = { code: found.code, at: m.receivedAt, from: [] };
  }
  return best && { code: best.code, from: best.from };
}

export interface CodeToastOptions {
  now?: number;
  /** How recent a text must be to count as a live arrival (default 2 min). */
  liveMs?: number;
  /** Window for matching an ANCS notification that already carried this code (default 10 min). */
  windowMs?: number;
  contacts?: Contact[];
}

/**
 * The code to pop up for a freshly arrived text, or null to stay quiet: an incoming, recent text
 * carrying a code that no ANCS notification has already delivered. The caller still applies the
 * toast settings, Do-not-disturb, the rate limiter and the once-per-code guard.
 */
export function codeToastForMessage(m: SmsMessage, notifications: PhoneNotification[], opts: CodeToastOptions = {}): string | null {
  const now = opts.now ?? Date.now();
  if (m.direction !== "in" || now - m.receivedAt > (opts.liveMs ?? DEFAULT_LIVE_MS)) return null;
  const found = findCode(m.body, m.address);
  if (!found) return null;
  const nameFor = new Map((opts.contacts ?? []).map((c) => [c.address, c.name]));
  const conversationKey = threadKey({ appId: MESSAGES_APP, title: senderName(m, nameFor) });
  const index = notificationCodes(notifications);
  if (coveredByNotification(index, conversationKey, found.code, m.receivedAt, opts.windowMs ?? SAME_MESSAGE_MS)) return null;
  return found.code;
}
