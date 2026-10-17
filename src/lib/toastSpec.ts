// What a Windows pop-up offers for one phone notification: a reply box for a text from a
// person tug can answer, Mark read for conversations, Copy code, Call back for a missed
// call, Clear for anything clearable. Whether it pops up at all is decided by the store
// (settings, mute, do-not-disturb, rate limit); the backend builds and handles the toast.

import type { Contact, PhoneNotification, SmsMessage, ToastSpec } from "../types/protocol";
import { appLabel, canClear, cleanName, groupConversations, isConversation, threadKey } from "./format";
import { findCode } from "./codes";

const MESSAGES_APP = "com.apple.MobileSMS";

/** iOS names a sender it has no contact for by their number or email. */
function addressInTitle(title: string): string | null {
  const t = cleanName(title);
  const digits = t.replace(/\D/g, "");
  if (/^\+?[\d\s().-]+$/.test(t) && digits.length >= 7) return t;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(t)) return t;
  return null;
}

/**
 * Whether a reply sent from a pop-up reads the conversation (seen here, read and cleared on the
 * phone), as replying on the phone would. Only a reply the phone took: a failed or unconfirmed one
 * leaves the conversation unread, with the notification still there to come back to. No stored
 * row (an older backend) keeps the old behaviour.
 */
export function replyReadsConversation(message: SmsMessage | null | undefined): boolean {
  return !message || message.status === "accepted" || message.status === "sent";
}

/**
 * How a send from tug's composer went, by the same rule: "sent" only when the phone took it (the
 * conversation's notifications can be cleared), "unconfirmed" when it may have sent but the phone
 * never said ("May have sent"), "failed" for "Not sent" with Retry.
 */
export function sendOutcome(message: SmsMessage): "sent" | "unconfirmed" | "failed" {
  if (message.status === "failed") return "failed";
  return replyReadsConversation(message) ? "sent" : "unconfirmed";
}

/**
 * Where a reply to this Messages notification goes. A pop-up's reply box doesn't show which
 * number it sends to, so it only gets one when there's exactly one: the conversation's only
 * number, or the number/email iOS shows for an unknown sender. When the name has several numbers
 * (two people with the same name, or one person with two phones) `to` is null and `ambiguous`
 * is set: the pop-up offers "Open in tug" instead, where the number is shown and can be picked.
 */
export function replyTarget(
  n: PhoneNotification,
  messages: SmsMessage[],
  contacts: Contact[],
): { to: string | null; ambiguous: boolean } {
  if (n.appId !== MESSAGES_APP || !isConversation(n)) return { to: null, ambiguous: false };
  const key = threadKey(n);
  const conversation = groupConversations([n], messages, contacts).find((c) => c.key === key);
  if (conversation && conversation.addresses.length > 1) return { to: null, ambiguous: true };
  return { to: conversation?.address ?? addressInTitle(n.title), ambiguous: false };
}

/** The one number a reply to this notification would go to, or null (see `replyTarget`). */
export function replyAddress(n: PhoneNotification, messages: SmsMessage[], contacts: Contact[]): string | null {
  return replyTarget(n, messages, contacts).to;
}

export function toastSpec(n: PhoneNotification, messages: SmsMessage[], contacts: Contact[]): ToastSpec {
  const conversation = isConversation(n);
  const ringing = n.category === "incomingCall";
  const reply = ringing ? { to: null, ambiguous: false } : replyTarget(n, messages, contacts);
  return {
    id: n.id,
    title: [appLabel(n), n.title].filter(Boolean).join(" · "),
    body: [n.subtitle, n.message].filter(Boolean).join("\n"),
    name: cleanName(n.title),
    replyTo: reply.to,
    open: reply.ambiguous,
    markRead: conversation && !ringing,
    code: findCode(n.message || n.subtitle)?.code ?? null,
    callBack: n.category === "missedCall" && n.live && n.removedAt == null && n.flags.positiveAction,
    clear: canClear(n),
  };
}
