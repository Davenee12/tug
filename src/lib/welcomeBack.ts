// "Welcome back": coming back to the PC after a while, one card at the top of the Feed sums up what
// arrived meanwhile (texts by person, missed calls, codes, other apps' notifications), so the
// person doesn't have to scroll through everything.
//
// Two parts, both pure (no DOM, no Tauri, no clock of their own: callers pass `now`):
//   • `AwayClock` decides when someone has been away and is back (the store feeds it window events);
//   • `welcomeBack` builds the summary from what tug already holds. It groups the way the rest of tug
//     does: texts by conversation (`groupConversations`, so a text that came as both a notification
//     and a message counts once, and a row opens the same conversation the Feed does), other chat
//     apps by sender (`threadKey`), everything else by app (the Feed's stacks). "Seen" is the Feed's
//     own: `ui.seen` per row key, and anything cleared on the phone or watch was read there.

import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";
import { findCode } from "./codes";
import { CODE_FILL_MAX_AGE_MS } from "./codeFill";
import {
  MESSAGES_APP,
  appLabel,
  cleanName,
  clockTime,
  dayLabel,
  groupConversations,
  isChatMessage,
  isConversation,
  messageTime,
  nameKey,
  notificationTime,
  senderName,
  threadKey,
  type Conversation,
  type ConversationItem,
} from "./format";
import { itemContent, previewOf } from "./messageType";

/** Away at least this long before coming back says "welcome back". */
export const AWAY_MIN_MS = 20 * 60_000;
/** Fewer new things than this and the Feed itself is summary enough: no card. */
export const MIN_ITEMS = 3;
/** Rows the card shows before "and N more below". */
export const MAX_GROUPS = 6;
/** A code older than this has likely expired: counted, its digits not shown (as code fill). */
export const CODE_FRESH_MS = CODE_FILL_MAX_AGE_MS;
/** A row's line is cut to about this many characters (the row truncates further by width). */
export const LINE_MAX = 80;
/**
 * How long the card must have been on screen, with someone looking, before scrolling or clicking
 * elsewhere in the Feed puts it away. Coming back often starts with a scroll; that one mustn't.
 */
export const SEEN_AFTER_MS = 2_000;
/** A text and its notification are the same one within this window (as `groupConversations`). */
const SAME_TEXT_MS = 10 * 60_000;
/** Texts this far outside the absence are grouped too, so one near the edge still finds its twin. */
const MATCH_MARGIN_MS = 15 * 60_000;

/** When the person was away: from `since` to `until` (unix ms). */
export interface Absence {
  since: number;
  until: number;
}

/**
 * Has the person been away, and are they back?
 *
 * The signal is tug's own window, because that's all a web page can see (Windows' idle time
 * would need the backend). "Here" is any sign of someone using the window: it comes to the front
 * (focus, or shown from the tray), or a pointer or key event lands in it. "Left" is the window
 * hidden (to the tray, minimised, often the screen locking) or losing focus. Away is the gap from
 * the last moment they were here to the next sign of them. Leaving counts as the last moment
 * (until then they were at the window), and with the window in front but untouched, the last input
 * is. That covers walking away with tug hidden, in the background or left open in front.
 *
 * It errs short, never long: a window left on screen may have been read without a touch. And with
 * no observed start (tug started hidden in the tray at sign-in), there's no absence to report:
 * how long they'd really been away is unknown, and the card never guesses.
 */
export class AwayClock {
  private lastHere: number | null;
  private present: boolean;

  /** `present`: the window is on screen right now (someone just opened it). */
  constructor(now: number, present: boolean, private readonly minAwayMs = AWAY_MIN_MS) {
    this.present = present;
    this.lastHere = present ? now : null;
  }

  /** Someone is using the window now. Returns the absence this ends, if it was long enough. */
  here(now: number): Absence | null {
    const since = this.lastHere;
    this.lastHere = now;
    this.present = true;
    if (since === null || now - since < this.minAwayMs) return null;
    return { since, until: now };
  }

  /** The window was hidden or lost focus: they were here until now. */
  left(now: number): void {
    if (this.present) this.lastHere = now;
    this.present = false;
  }
}

/** Where a row leads: a Messages conversation, or a notification's row in the Feed. */
export type WelcomeTarget = { kind: "conversation"; key: string } | { kind: "feed"; item: string };

export interface WelcomeGroup {
  /** Unique within the card. */
  key: string;
  /** person: texts or a chat app's messages from someone; call: missed calls from someone; code: a
   * sender who only sent verification codes (a bank, a short code); app: an app's notifications. */
  kind: "person" | "call" | "code" | "app";
  /** The person, caller or sender as tug names them elsewhere; the app's name for an app. */
  name: string;
  appId: string;
  appLabel: string;
  /** How many arrived. */
  count: number;
  /** The newest one, in one line (a code that's likely expired is said as a count, never its digits). */
  line: string;
  /** When the newest one came (unix ms). */
  at: number;
  /** A code still fresh enough to use (at most `CODE_FRESH_MS` old), to copy from the row. */
  code: string | null;
  /** The notification(s) that code came on, cleared once it's copied (as the Feed's Copy code). */
  codeFrom: PhoneNotification[];
  /** Codes in this group older than that: counted, digits not shown. */
  staleCodes: number;
  target: WelcomeTarget;
}

export interface WelcomeCounts {
  /** Texts (Messages) from people. */
  texts: number;
  /** Other chat apps' messages from people (WhatsApp, Teams…). */
  chats: number;
  /** People those came from. */
  people: number;
  missedCalls: number;
  /** Messages from senders that only sent codes. */
  codes: number;
  /** Everything else, from apps. */
  notifications: number;
}

export interface WelcomeBack extends Absence {
  /** "1 h 20 min" */
  awayFor: string;
  /** "While you were away (1 h 20 min): 7 texts from 3 people, 2 missed calls, 12 notifications" */
  headline: string;
  /** People (with calls and code senders) newest first, then apps newest first; at most ~MAX_GROUPS. */
  groups: WelcomeGroup[];
  /** Groups left out of `groups` ("and N more below"). */
  more: number;
  /** Everything counted. */
  total: number;
  counts: WelcomeCounts;
}

export interface WelcomeInput extends Absence {
  now: number;
  notifications: PhoneNotification[];
  /** Texts read over message access; outgoing ones (sent from tug or the phone) never count. */
  messages: SmsMessage[];
  contacts: Contact[];
  /** Everything under this Feed or conversation key received at or before this time has been seen. */
  seenAt: (key: string) => number;
  /** Whether a Messages conversation is a known sender's (Filter unknown senders). Default: all are. */
  isKnown?: (c: Pick<Conversation, "appId" | "contact" | "addresses">) => boolean;
  /** Code texts cleared from the Feed (hidden there): those are dealt with. */
  clearedCodes?: Iterable<number>;
}

/** "1 text", "3 texts"; `many` for irregular plurals ("people"). */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** How long someone was away, in words: "45 min", "1 h 20 min", "2 h", "1 day 3 h". */
export function awayFor(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
  const days = Math.floor(hours / 24);
  return hours % 24 ? `${plural(days, "day")} ${hours % 24} h` : plural(days, "day");
}

/** One line of at most `max` characters: whitespace folded, cut at a word with "…" when it can be. */
export function oneLine(text: string, max = LINE_MAX): string {
  const flat = text.replace(/\s+/g, " ").trim();
  // By code point, so an emoji is never cut in half.
  const chars = Array.from(flat);
  if (chars.length <= max) return flat;
  const cut = chars.slice(0, max - 1).join("");
  const space = cut.lastIndexOf(" ");
  const kept = space > cut.length * 0.6 ? cut.slice(0, space) : cut;
  return `${kept.replace(/[\s,;:.–—-]+$/u, "")}…`;
}

/** A row's time: the clock time today, with the day before that ("Yesterday 11:40 PM"). */
export function whenLabel(at: number, now: number): string {
  const d = new Date(at);
  const day = dayLabel(d, new Date(now));
  return day === "Today" ? clockTime(d) : `${day} ${clockTime(d)}`;
}

/** One thing that arrived while they were away. */
interface Arrival {
  groupKey: string;
  kind: WelcomeGroup["kind"];
  name: string;
  appId: string;
  appLabel: string;
  at: number;
  receivedAt: number;
  line: string;
  /** A one-time code in it, if any. */
  code: string | null;
  /** The notification behind it, still on the phone (for a code's Copy, and the Feed's row). */
  n: PhoneNotification | null;
  target: WelcomeTarget;
}

const stackKey = (appId: string) => `app\u0000${appId}`;

/** The notification a conversation item stands for (a text's twin, matched as `groupConversations` does). */
function twinOf(c: Conversation, item: ConversationItem): PhoneNotification | undefined {
  if (item.kind === "notification") return item.n;
  const body = item.body.trim();
  return c.notifications.find(
    (n) => (n.message || n.subtitle).trim() === body && Math.abs(notificationTime(n).getTime() - item.at.getTime()) < SAME_TEXT_MS,
  );
}

/** Texts, by conversation: Messages notifications and message-access texts, each text once. */
function textArrivals(input: WelcomeInput, inWindow: (at: number) => boolean, cleared: Set<number>): Arrival[] {
  const near = (at: number) => at > input.since - MATCH_MARGIN_MS && at <= input.until + MATCH_MARGIN_MS;
  // Only what's near the absence: the whole history would be grouped for nothing.
  const notes = input.notifications.filter((n) => isConversation(n) && near(notificationTime(n).getTime()));
  // Sent texts never count, but one sent since (from the phone or tug) says what came before it was read.
  const texts = input.messages.filter((m) => {
    const at = messageTime(m).getTime();
    return m.direction === "in" ? near(at) : at > input.since - MATCH_MARGIN_MS;
  });
  const out: Arrival[] = [];
  for (const c of groupConversations(notes, texts, input.contacts)) {
    const seenAt = input.seenAt(c.key);
    const known = input.isKnown?.(c) ?? true;
    let repliedAt = -Infinity;
    for (const item of c.items) if (item.kind === "message" && item.m.direction === "out") repliedAt = Math.max(repliedAt, item.at.getTime());
    for (const item of c.items) {
      const at = item.at.getTime();
      if (!inWindow(at) || at <= repliedAt) continue;
      const twin = twinOf(c, item);
      // Cleared on the phone or watch (or, for a code text, in the Feed): already dealt with.
      if (twin && twin.removedAt != null) continue;
      if (item.kind === "message" && (item.m.direction !== "in" || cleared.has(item.m.id))) continue;
      const receivedAt = item.kind === "message" ? item.m.receivedAt : item.n.receivedAt;
      if (receivedAt <= seenAt) continue;
      const code = findCode(item.body)?.code ?? null;
      // Filter unknown senders: a stranger's texts wait in their own list, but a code still matters.
      if (!known && !code) continue;
      out.push({
        groupKey: c.key,
        kind: "person",
        name: c.contact,
        appId: MESSAGES_APP,
        appLabel: c.appLabel,
        at,
        receivedAt,
        line: previewOf(itemContent(item)),
        code,
        n: twin ?? null,
        target: { kind: "conversation", key: c.key },
      });
    }
  }
  return out;
}

/** Everything else on the phone: missed calls by caller, other chat apps by sender, the rest by app. */
function otherArrivals(input: WelcomeInput, inWindow: (at: number) => boolean): Arrival[] {
  const out: Arrival[] = [];
  for (const n of input.notifications) {
    // Texts are counted by conversation above; a ringing call is over by now (a missed one has its own).
    if (isConversation(n) || n.category === "incomingCall" || n.removedAt != null) continue;
    const at = notificationTime(n).getTime();
    if (!inWindow(at) || n.receivedAt <= input.seenAt(stackKey(n.appId))) continue;
    const label = appLabel(n);
    const body = n.message || n.subtitle;
    const base = { appId: n.appId, appLabel: label, at, receivedAt: n.receivedAt, code: findCode(body)?.code ?? null, n };
    const target: WelcomeTarget = { kind: "feed", item: `n${n.id}` };
    if (n.category === "missedCall") {
      const caller = senderName(n.title) || "No caller ID";
      out.push({ ...base, groupKey: `call\u0000${n.appId}\u0000${nameKey(caller)}`, kind: "call", name: caller, line: "Missed call", target });
    } else if (isChatMessage(n) && !cleanName(n.title).startsWith("#")) {
      // Someone on WhatsApp, Teams… (a "#channel" is a place, not a person: it stays with its app).
      out.push({ ...base, groupKey: threadKey(n), kind: "person", name: cleanName(n.title), line: body || "(no preview)", target });
    } else {
      const line = [n.title, body].filter(Boolean).join(" · ") || "(no preview)";
      out.push({ ...base, groupKey: stackKey(n.appId), kind: "app", name: label, line, target });
    }
  }
  return out;
}

/** Arrivals as rows: newest decides the line, time and target; codes fresh or counted. */
function toGroups(arrivals: Arrival[], now: number): WelcomeGroup[] {
  const byKey = new Map<string, Arrival[]>();
  for (const a of arrivals) {
    const list = byKey.get(a.groupKey);
    if (list) list.push(a);
    else byKey.set(a.groupKey, [a]);
  }
  const groups: WelcomeGroup[] = [];
  for (const [key, list] of byKey) {
    list.sort((a, b) => b.at - a.at);
    const newest = list[0];
    const fresh = (a: Arrival) => a.code !== null && now - a.receivedAt <= CODE_FRESH_MS;
    const freshCode = list.find(fresh) ?? null;
    const staleCodes = list.filter((a) => a.code !== null && !fresh(a)).length;
    // Someone who only sent codes isn't a conversation to catch up on: a bank, a sign-in.
    const kind = newest.kind === "person" && list.every((a) => a.code !== null) ? "code" : newest.kind;
    let line = newest.line;
    if (newest.kind === "call") line = list.length > 1 ? plural(list.length, "missed call") : "Missed call";
    else if (newest.code !== null && !fresh(newest)) line = staleCodes > 1 ? plural(staleCodes, "verification code") : "Verification code";
    groups.push({
      key,
      kind,
      name: newest.name,
      appId: newest.appId,
      appLabel: newest.appLabel,
      count: list.length,
      line: oneLine(line),
      at: newest.at,
      code: freshCode?.code ?? null,
      codeFrom: freshCode?.n ? [freshCode.n] : [],
      staleCodes,
      target: newest.target,
    });
  }
  // People first (and callers and code senders, who are people-ish), newest first; then apps.
  const rank = (g: WelcomeGroup) => (g.kind === "app" ? 1 : 0);
  return groups.sort((a, b) => rank(a) - rank(b) || b.at - a.at);
}

function countsOf(groups: WelcomeGroup[]): WelcomeCounts {
  const counts: WelcomeCounts = { texts: 0, chats: 0, people: 0, missedCalls: 0, codes: 0, notifications: 0 };
  for (const g of groups) {
    if (g.kind === "person") {
      counts.people += 1;
      if (g.appId === MESSAGES_APP) counts.texts += g.count;
      else counts.chats += g.count;
    } else if (g.kind === "call") counts.missedCalls += g.count;
    else if (g.kind === "code") counts.codes += g.count;
    else counts.notifications += g.count;
  }
  return counts;
}

/** "While you were away (1 h 20 min): 7 texts from 3 people, 2 missed calls, 12 notifications" */
export function headline(away: string, c: WelcomeCounts): string {
  const parts: string[] = [];
  const said = c.texts + c.chats;
  if (said) {
    // "Texts" when they're all texts; with WhatsApp and the like in it, "messages".
    const what = plural(said, c.chats ? "message" : "text");
    parts.push(said === 1 ? what : `${what} from ${plural(c.people, "person", "people")}`);
  }
  if (c.missedCalls) parts.push(plural(c.missedCalls, "missed call"));
  if (c.codes) parts.push(plural(c.codes, "verification code"));
  if (c.notifications) parts.push(plural(c.notifications, "notification"));
  return `While you were away (${away})${parts.length ? `: ${parts.join(", ")}` : ""}`;
}

/**
 * What arrived from `since` to `until` that's still waiting: not seen in tug, not cleared on the
 * phone, not sent by you. Computed again as things change, so a row the person opens (seen) or
 * clears drops off the card; `now` ages codes.
 */
export function welcomeBack(input: WelcomeInput): WelcomeBack {
  const inWindow = (at: number) => at > input.since && at <= input.until;
  const arrivals = [
    ...textArrivals(input, inWindow, new Set(input.clearedCodes ?? [])),
    ...otherArrivals(input, inWindow),
  ];
  const all = toGroups(arrivals, input.now);
  // Never "and 1 more": one more row takes the same room as saying so.
  const groups = all.length > MAX_GROUPS + 1 ? all.slice(0, MAX_GROUPS) : all;
  const counts = countsOf(all);
  const away = awayFor(input.until - input.since);
  return {
    since: input.since,
    until: input.until,
    awayFor: away,
    headline: headline(away, counts),
    groups,
    more: all.length - groups.length,
    total: arrivals.length,
    counts,
  };
}

/** The iPhone's Phone app: its missed calls need no app name beside them. */
const PHONE_APP = "com.apple.mobilephone";

/**
 * The small words beside a row's name: the app when it isn't obvious (WhatsApp, not Messages or
 * Phone), and how many when there's more than one ("3 texts", "WhatsApp · 2 messages"). A call's or
 * a code sender's count is already in its line.
 */
export function rowMeta(g: Pick<WelcomeGroup, "kind" | "appId" | "appLabel" | "count">): string {
  const parts: string[] = [];
  if (g.kind !== "app" && g.appId !== MESSAGES_APP && g.appId !== PHONE_APP) parts.push(g.appLabel);
  if (g.count > 1 && (g.kind === "person" || g.kind === "app")) {
    const what = g.kind === "app" ? "notification" : g.appId === MESSAGES_APP ? "text" : "message";
    parts.push(plural(g.count, what));
  }
  return parts.join(" · ");
}

/** Whether coming back after `absence` deserves the card: away long enough, and enough waiting. */
export function worthACard(summary: Pick<WelcomeBack, "since" | "until" | "total">): boolean {
  return summary.until - summary.since >= AWAY_MIN_MS && summary.total >= MIN_ITEMS;
}
