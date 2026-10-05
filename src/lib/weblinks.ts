// Web links for notifications. iOS gives us no deep link to the exact item, so we map a
// bundle id to the most useful landing page for that app on the web (inbox, notifications,
// feed…). Gmail opens the inbox: a search built from the notification's sender and subject
// often found nothing (iOS's sender name and subject rarely match Gmail's search exactly).
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
const GOOGLE_APP_ID = "com.google.GoogleMobile";
/** Apps whose notifications are the phone itself, with their own actions (reply, call back). */
const NO_WEB = new Set(["com.apple.MobileSMS", "com.apple.mobilephone", "com.apple.facetime"]);


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
  [GMAIL_APP_ID]: { url: "https://mail.google.com/mail/u/0/#inbox", label: "Gmail" },
};

/**
 * Where this notification's app opens on the web, or null when we have no useful page for it
 * (Messages, Phone, and any app not in the map).
 */
export function webLinkFor(n: PhoneNotification): WebLink | null {
  // The Google app's alerts (live scores, news) are about something: search for it.
  if (n.appId === GOOGLE_APP_ID) {
    const topic = (n.subtitle || n.message.split(/\r?\n/)[0] || n.title).split(" · ")[0].replace(/\s+/g, " ").trim();
    return {
      url: topic ? `https://www.google.com/search?q=${encodeURIComponent(topic.slice(0, 120))}` : "https://www.google.com/",
      label: "Google",
    };
  }
  return LANDING[n.appId] ?? null;
}

/** Whether an app can have a website to open at all (not Messages, Phone or FaceTime). */
export function mayHaveWebsite(appId: string): boolean {
  return !NO_WEB.has(appId);
}
