// Web links for notifications. iOS gives us no deep link to the exact item, so we map a
// bundle id to the most useful landing page for that app on the web (inbox, notifications,
// feed…). Gmail is special-cased into a deep search built from the sender and subject.
//
// Pure module: no Tauri, no DOM. The UI calls `webLinkFor` to decide whether to show an
// "Open" button and where it points; see `src/lib/ipc.ts` `openUrl` for the actual launch.

import type { PhoneNotification } from "../types/protocol";

export interface WebLink {
  /** An http(s) URL the backend will open in the default browser. */
  url: string;
  /** The app the link lands on, for the button tooltip ("Open in Gmail on the web"). */
  label: string;
}

export const GMAIL_APP_ID = "com.google.Gmail";

const GMAIL_SEARCH = "https://mail.google.com/mail/u/0/#search/";
const GMAIL_INBOX = "https://mail.google.com/mail/u/0/#inbox";

/** Longest Gmail query we build; keeps the URL sane when a subject line runs long. */
const GMAIL_QUERY_MAX = 120;

/**
 * Static landing pages, by iOS bundle id. Each is the page a person most likely wants after
 * tapping that app's notification — its inbox, notifications tab, or feed. Bundle ids are the
 * real App Store ones (several apps keep historical ids: X is still `com.atebits.Tweetie2`,
 * Snapchat `com.toyopagroup.picaboo`). Apps not listed here get no "Open" button.
 */
const LANDING: Record<string, WebLink> = {
  "com.linkedin.LinkedIn": { url: "https://www.linkedin.com/notifications/", label: "LinkedIn" },
  "com.burbn.instagram": { url: "https://www.instagram.com/direct/inbox/", label: "Instagram" },
  "com.facebook.Facebook": { url: "https://www.facebook.com/notifications", label: "Facebook" },
  "com.facebook.Messenger": { url: "https://www.messenger.com/", label: "Messenger" },
  "com.atebits.Tweetie2": { url: "https://x.com/notifications", label: "X" },
  "com.microsoft.Office.Outlook": { url: "https://outlook.office.com/mail/", label: "Outlook" },
  "com.tinyspeck.chatlyio": { url: "https://app.slack.com/client", label: "Slack" },
  "com.google.ios.youtube": { url: "https://www.youtube.com/feed/subscriptions", label: "YouTube" },
  "com.reddit.Reddit": { url: "https://www.reddit.com/notifications/", label: "Reddit" },
  "com.toyopagroup.picaboo": { url: "https://web.snapchat.com/", label: "Snapchat" },
  "net.whatsapp.WhatsApp": { url: "https://web.whatsapp.com/", label: "WhatsApp" },
  "com.hammerandchisel.discord": { url: "https://discord.com/channels/@me", label: "Discord" },
  "ph.telegra.Telegraph": { url: "https://web.telegram.org/a/", label: "Telegram" },
  "com.google.calendar": { url: "https://calendar.google.com/", label: "Google Calendar" },
  "com.amazon.Amazon": { url: "https://www.amazon.com/gp/css/order-history", label: "Amazon" },
  "com.github.stormbreaker.prod": { url: "https://github.com/notifications", label: "GitHub" },
  "com.spotify.client": { url: "https://open.spotify.com/", label: "Spotify" },
  "notion.id": { url: "https://www.notion.so/", label: "Notion" },
  "com.zhiliaoapp.musically": { url: "https://www.tiktok.com/", label: "TikTok" },
  "com.google.Voice": { url: "https://voice.google.com/", label: "Google Voice" },
};

/** Drop double quotes (Gmail search has no escape for them) and collapse whitespace. */
function clean(s: string | null | undefined): string {
  return (s ?? "").replace(/"/g, "").replace(/\s+/g, " ").trim();
}

/**
 * The Gmail search for a notification, e.g. `from:"Jane Doe" "Your order shipped"`. The sender
 * is the notification title; the subject is the subtitle when there is one, else the first line
 * of the message (Gmail puts the subject there, sometimes with the snippet below). Returns an
 * empty string when there's nothing to search on, so the caller falls back to the inbox.
 */
export function gmailQuery(n: Pick<PhoneNotification, "title" | "subtitle" | "message">): string {
  const sender = clean(n.title);
  const subject = clean(n.subtitle) || clean(n.message.split(/\r?\n/)[0]);
  const parts: string[] = [];
  if (sender) parts.push(`from:"${sender}"`);
  if (subject) parts.push(`"${subject}"`);
  return parts.join(" ").slice(0, GMAIL_QUERY_MAX).trim();
}

function gmailLink(n: PhoneNotification): WebLink {
  const query = gmailQuery(n);
  return { url: query ? GMAIL_SEARCH + encodeURIComponent(query) : GMAIL_INBOX, label: "Gmail" };
}

/**
 * Where this notification's app opens on the web, or null when we have no useful page for it
 * (Messages, Phone, and any app not in the map). Gmail gets a deep search; everything else a
 * fixed landing page.
 */
export function webLinkFor(n: PhoneNotification): WebLink | null {
  if (n.appId === GMAIL_APP_ID) return gmailLink(n);
  return LANDING[n.appId] ?? null;
}
