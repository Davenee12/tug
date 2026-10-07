// iMessage vs text (blue vs green), from the MAP message `type` the phone reports.
//
// iOS lists each message as SMS_GSM, SMS_CDMA, MMS, EMAIL or IM. "IM" is iMessage (blue); the
// SMS/MMS kinds are plain texts (green). tug only tints when the phone actually makes the
// distinction — i.e. when both an iMessage and a plain text have been seen. If every message comes
// back the same type (all IM, all SMS, or all untyped), that tells us nothing, so the bubbles keep
// today's neutral look rather than guessing a colour.

import type { SmsMessage } from "../types/protocol";

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

/** Shown for a text with no words: a photo or other attachment the phone won't send over. */
export const ATTACHMENT_ONLY = "Photo or attachment — open it on your iPhone";

/**
 * What to show for a text. The phone sends no text for a photo or other attachment (MAP leaves
 * attachments out), so an empty MMS or untyped text says so instead of "(no preview)". An empty
 * plain text or iMessage is just empty.
 */
export function messageText(m: Pick<SmsMessage, "body" | "msgType">): string {
  if (m.body) return m.body;
  const t = (m.msgType ?? "").toUpperCase();
  return t === "MMS" || classifyType(m.msgType) === "unknown" ? ATTACHMENT_ONLY : "(no preview)";
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
