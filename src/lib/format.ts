import type { PhoneNotification } from "../types/protocol";

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
  items: PhoneNotification[];
  latest: PhoneNotification;
}

/**
 * Group messaging notifications into conversations by app + sender.
 * ANCS only carries what was on the lock screen, so these are incoming-only.
 */
export function groupThreads(notifications: PhoneNotification[]): Thread[] {
  const threads = new Map<string, Thread>();
  for (const n of notifications) {
    if (!(n.appId in MESSAGING_APPS) || !n.title) continue;
    const key = `${n.appId}\u0000${n.title}`;
    const t = threads.get(key);
    if (t) {
      t.items.push(n);
      if (notificationTime(n) > notificationTime(t.latest)) t.latest = n;
    } else {
      threads.set(key, { key, appId: n.appId, appLabel: appLabel(n), contact: n.title, items: [n], latest: n });
    }
  }
  for (const t of threads.values()) {
    t.items.sort((a, b) => notificationTime(a).getTime() - notificationTime(b).getTime());
  }
  return [...threads.values()].sort(
    (a, b) => notificationTime(b.latest).getTime() - notificationTime(a.latest).getTime(),
  );
}
