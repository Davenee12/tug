// iMessage vs text (blue vs green), from the MAP message `type` the phone reports.
//
// iOS lists each message as SMS_GSM, SMS_CDMA, MMS, EMAIL or IM. "IM" is iMessage (blue); the
// SMS/MMS kinds are plain texts (green). tug only tints when the phone actually makes the
// distinction — i.e. when both an iMessage and a plain text have been seen. If every message comes
// back the same type (all IM, all SMS, or all untyped), that tells us nothing, so the bubbles keep
// today's neutral look rather than guessing a colour.

import type { SmsMessage } from "../types/protocol";
import { isUnreadable, parseAttachments, previewLine, UNREADABLE, type Media } from "./attachments";
import { MESSAGES_APP, type ConversationItem } from "./format";

export type MessageKind = "imessage" | "sms" | "unknown";

/** Which family a MAP type string belongs to. Unknown/empty types (and EMAIL) stay neutral. */
export function classifyType(msgType: string | null | undefined): MessageKind {
  switch ((msgType ?? "").toUpperCase()) {
    case "IM":
      return "imessage";
    case "SMS_GSM":
    case "SMS_CDMA":
    case "MMS":
      return "sms";
    default:
      return "unknown";
  }
}

/**
 * Whether the phone distinguishes iMessage from text in what tug has seen: both an IM and an
 * SMS/MMS must be present. Until then there's nothing to colour.
 */
export function distinguishesIMessage(messages: Pick<SmsMessage, "msgType">[]): boolean {
  let hasIMessage = false;
  let hasSms = false;
  for (const m of messages) {
    const kind = classifyType(m.msgType);
    if (kind === "imessage") hasIMessage = true;
    else if (kind === "sms") hasSms = true;
    if (hasIMessage && hasSms) return true;
  }
  return false;
}

/** What a text (or a Messages notification) shows: its words, a media chip, or both. */
export interface Shown {
  /** The words (a caption beside a chip); "" for a photo on its own. */
  text: string;
  /** Attachments to show as a chip ("Photo · Open it on your iPhone"), or null. */
  media: Media | null;
  /** The phone sent no readable characters (only U+FFFD): say so instead of showing "�". */
  unreadable: boolean;
}

const plain = (text: string): Shown => ({ text, media: null, unreadable: false });

/**
 * A received text's body or a Messages notification's message: "Attachment: 1 Photo" becomes a
 * chip (with any caption kept), a body of only U+FFFD reads as "didn't come through", and
 * anything else is shown exactly as it came.
 */
export function textContent(body: string): Shown {
  if (isUnreadable(body)) return { text: "", media: null, unreadable: true };
  const p = parseAttachments(body);
  return { text: p.text, media: p.media, unreadable: false };
}

type MessageLike = Pick<SmsMessage, "body" | "msgType"> &
  Partial<Pick<SmsMessage, "attachments" | "unsent">> & { direction?: SmsMessage["direction"] };

/** What a text that was unsent on the phone says in its place (history from a backup). */
export const UNSENT = "This message was unsent.";

/** The media a text's backup attachments add up to: one kind and how many, or "attachment" for a mix. */
function attachmentsMedia(atts: NonNullable<SmsMessage["attachments"]>): Media {
  const kinds = new Set(atts.map((a) => a.kind));
  const only = kinds.size === 1 ? atts[0].kind : "attachment";
  return { kind: only, count: atts.length };
}

/**
 * What to show for a text. A photo arrives as words ("Attachment: 1 Photo", see lib/attachments).
 * An empty MMS or untyped text is an attachment the phone said nothing about (MAP leaves
 * attachments out), so it's a "Photo or attachment" chip; an empty plain text or iMessage is just
 * empty. Texts sent from tug are shown as typed. `body` overrides the stored body (a conversation
 * may fill an unreadable text from its notification).
 */
export function messageContent(m: MessageLike, body = m.body): Shown {
  if (m.unsent) return plain(UNSENT);
  // History from a backup knows its real attachments: Bluetooth's "Attachment: 1 Photo" line (on
  // a text the import matched) gives way to them, and the caption stays.
  if (m.attachments?.length) {
    const text = m.direction === "out" ? body : parseAttachments(body).text;
    return { text, media: attachmentsMedia(m.attachments), unreadable: false };
  }
  if (m.direction === "out") return plain(body);
  if (!body) {
    const t = (m.msgType ?? "").toUpperCase();
    return t === "MMS" || classifyType(m.msgType) === "unknown"
      ? { text: "", media: { kind: "attachment", count: null }, unreadable: false }
      : plain("(no preview)");
  }
  return textContent(body);
}

/** One item of a conversation. Other chat apps' notifications are shown as they are. */
export function itemContent(item: ConversationItem): Shown {
  if (item.kind === "message") return messageContent(item.m, item.body);
  if (item.n.appId === MESSAGES_APP) return textContent(item.body);
  return plain(item.body || "(no preview)");
}

/** One line for previews: "📷 Photo", "📷 " + caption, or "This text didn't come through". */
export function previewOf(s: Shown): string {
  if (s.unreadable) return UNREADABLE;
  return previewLine(s);
}

/** A text as one line of words (conversation lists, search). */
export function messageText(m: MessageLike): string {
  return previewOf(messageContent(m));
}

/**
 * The colour family for one message's bubble, or null to keep the neutral look. Only returns a
 * colour when the phone distinguishes the two and this message's own type is known.
 */
export function bubbleKind(m: Pick<SmsMessage, "msgType">, distinguishes: boolean): "imessage" | "sms" | null {
  if (!distinguishes) return null;
  const kind = classifyType(m.msgType);
  return kind === "unknown" ? null : kind;
}
