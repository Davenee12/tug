import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

/** Bundle ids whose notifications are conversations, grouped as threads. */
export const MESSAGING_APPS: Record<string, string> = {
  "com.apple.MobileSMS": "Messages",
  "net.whatsapp.WhatsApp": "WhatsApp",
  "ph.telegra.Telegraph": "Telegram",
  "org.whispersystems.signal": "Signal",
  "com.facebook.Messenger": "Messenger",
  "com.hammerandchisel.discord": "Discord",
  "com.tinyspeck.chatlyio": "Slack",
  "com.microsoft.skype.teams": "Teams",
};

export function appLabel(n: Pick<PhoneNotification, "appId" | "appName">): string {
  if (n.appName) return n.appName;
  if (MESSAGING_APPS[n.appId]) return MESSAGING_APPS[n.appId];
  const last = n.appId.split(".").pop() ?? n.appId;
  return last.charAt(0).toUpperCase() + last.slice(1);
}

export function initials(label: string): string {
  const words = label.split(/[\s-]+/).filter(Boolean);
  const letters = words.length > 1 ? words[0][0] + words[1][0] : label.slice(0, 2);
  return letters.toUpperCase();
}

/** Stable warm tone per app so the feed is scannable without real app icons. */
const AVATAR_TONES = [
  "bg-primary/15 text-primary-active",
  "bg-accent-teal/20 text-ink",
  "bg-accent-amber/25 text-ink",
  "bg-surface-cream-strong text-body-strong",
  "bg-success/20 text-ink",
];

export function avatarTone(appId: string): string {
  let h = 0;
  for (const c of appId) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_TONES[h % AVATAR_TONES.length];
}

/** When the phone says it happened, falling back to when tug saw it. */
export function notificationTime(n: PhoneNotification): Date {
  if (n.postedAt) {
    const d = new Date(n.postedAt);
    if (!Number.isNaN(d.getTime())) return d;
  }
  return new Date(n.receivedAt);
}

export function clockTime(d: Date): string {
  return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function relativeTime(d: Date, now = Date.now()): string {
  const s = Math.round((now - d.getTime()) / 1000);
  if (s < 45) return "now";
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function dayLabel(d: Date, now = new Date()): string {
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86400000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return d.toLocaleDateString([], { weekday: "long" });
  return d.toLocaleDateString([], { month: "long", day: "numeric", year: "numeric" });
}

export function duration(seconds: number | null): string {
  if (seconds == null || !Number.isFinite(seconds)) return "–:––";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export interface Thread {
  key: string;
  appId: string;
  appLabel: string;
  contact: string;
  /** Oldest first, like a chat. */
  items: PhoneNotification[];
  latest: PhoneNotification;
}

/** Notifications from chat apps that name a sender are conversations. */
export function isConversation(n: PhoneNotification): boolean {
  return n.appId in MESSAGING_APPS && !!n.title;
}

/** One conversation per app + sender. */
export function threadKey(n: Pick<PhoneNotification, "appId" | "title">): string {
  return `${n.appId}\u0000${n.title}`;
}

const byTime = (a: PhoneNotification, b: PhoneNotification) =>
  notificationTime(a).getTime() - notificationTime(b).getTime();

/**
 * Group messaging notifications into conversations by app + sender.
 * ANCS only carries what was on the lock screen, so these are incoming-only.
 */
export function groupThreads(notifications: PhoneNotification[]): Thread[] {
  const threads = new Map<string, Thread>();
  for (const n of notifications) {
    if (!isConversation(n)) continue;
    const key = threadKey(n);
    const t = threads.get(key);
    if (t) {
      t.items.push(n);
      if (byTime(n, t.latest) > 0) t.latest = n;
    } else {
      threads.set(key, { key, appId: n.appId, appLabel: appLabel(n), contact: n.title, items: [n], latest: n });
    }
  }
  for (const t of threads.values()) t.items.sort(byTime);
  return [...threads.values()].sort((a, b) => byTime(b.latest, a.latest));
}

export interface AppStack {
  key: string;
  appId: string;
  appLabel: string;
  /** Newest first, like a notification stack. */
  items: PhoneNotification[];
  latest: PhoneNotification;
}

export type FeedEntry = { kind: "thread"; key: string; thread: Thread } | { kind: "stack"; key: string; stack: AppStack };

export function entryLatest(e: FeedEntry): PhoneNotification {
  return e.kind === "thread" ? e.thread.latest : e.stack.latest;
}

/**
 * The compact feed: one row per conversation, one collapsible stack per other
 * app, ordered by whichever has the newest notification.
 */
export function groupFeed(notifications: PhoneNotification[]): FeedEntry[] {
  const entries: FeedEntry[] = groupThreads(notifications).map((thread) => ({ kind: "thread", key: thread.key, thread }));
  const stacks = new Map<string, AppStack>();
  for (const n of notifications) {
    if (isConversation(n)) continue;
    const s = stacks.get(n.appId);
    if (s) {
      s.items.push(n);
      if (byTime(n, s.latest) > 0) s.latest = n;
    } else {
      stacks.set(n.appId, { key: `app\u0000${n.appId}`, appId: n.appId, appLabel: appLabel(n), items: [n], latest: n });
    }
  }
  for (const stack of stacks.values()) {
    stack.items.sort((a, b) => byTime(b, a));
    entries.push({ kind: "stack", key: stack.key, stack });
  }
  return entries.sort((a, b) => byTime(entryLatest(b), entryLatest(a)));
}

/** `+13025550173` → `(302) 555-0173`; anything else unchanged. */
export function formatAddress(address: string): string {
  const m = /^\+1(\d{3})(\d{3})(\d{4})$/.exec(address);
  return m ? `(${m[1]}) ${m[2]}-${m[3]}` : address;
}

const MESSAGES_APP = "com.apple.MobileSMS";

export type ConversationItem =
  | { kind: "notification"; id: string; at: Date; body: string; n: PhoneNotification }
  | { kind: "message"; id: string; at: Date; body: string; m: SmsMessage };

export interface Conversation {
  key: string;
  appId: string;
  appLabel: string;
  contact: string;
  /** Where replies go; known once message access has seen this person. */
  address: string | null;
  /** Oldest first. */
  items: ConversationItem[];
  latest: ConversationItem;
  /** The notification items, for new-message counts. */
  notifications: PhoneNotification[];
}

function messageTime(m: SmsMessage): Date {
  const d = m.sentAt ? new Date(m.sentAt) : null;
  return d && !Number.isNaN(d.getTime()) ? d : new Date(m.receivedAt);
}

/** A notification and a MAP message are the same text if body matches within this window. */
const SAME_MESSAGE_MS = 10 * 60 * 1000;

/**
 * Conversations for the Messages view: notifications from chat apps merged with
 * messages from message access (MAP). Keys match `threadKey`, so opening a feed
 * row lands on the merged conversation. A text reported by both is shown once.
 */
export function groupConversations(
  notifications: PhoneNotification[],
  messages: SmsMessage[],
  contacts: Contact[],
): Conversation[] {
  const nameFor = new Map(contacts.map((c) => [c.address, c.name]));
  const addressFor = new Map(contacts.map((c) => [c.name, c.address]));
  const convs = new Map<string, Conversation>();
  const get = (appId: string, label: string, contact: string): Conversation => {
    const key = threadKey({ appId, title: contact });
    let c = convs.get(key);
    if (!c) {
      const placeholder = { kind: "message", id: "", at: new Date(0), body: "" } as unknown as ConversationItem;
      c = { key, appId, appLabel: label, contact, address: null, items: [], latest: placeholder, notifications: [] };
      convs.set(key, c);
    }
    return c;
  };

  for (const m of messages) {
    const name = m.contactName ?? nameFor.get(m.address) ?? formatAddress(m.address);
    const c = get(MESSAGES_APP, "Messages", name);
    c.address = m.address;
    c.items.push({ kind: "message", id: `m${m.id}`, at: messageTime(m), body: m.body, m });
  }
  for (const n of notifications) {
    if (!isConversation(n)) continue;
    const c = get(n.appId, appLabel(n), n.title);
    if (n.appId === MESSAGES_APP) c.address ??= addressFor.get(n.title) ?? null;
    c.notifications.push(n);
    const at = notificationTime(n);
    const body = n.message || n.subtitle;
    const duplicate = c.items.some(
      (i) => i.kind === "message" && i.m.direction === "in" && i.body.trim() === body.trim() && Math.abs(i.at.getTime() - at.getTime()) < SAME_MESSAGE_MS,
    );
    if (!duplicate) c.items.push({ kind: "notification", id: `n${n.id}`, at, body, n });
  }

  const out = [...convs.values()].filter((c) => c.items.length > 0);
  for (const c of out) {
    c.items.sort((a, b) => a.at.getTime() - b.at.getTime());
    c.latest = c.items[c.items.length - 1];
  }
  return out.sort((a, b) => b.latest.at.getTime() - a.latest.at.getTime());
}
