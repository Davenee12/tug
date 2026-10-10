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
 * Where a reply to this Messages notification goes: the conversation's number (never a
 * guess between several), or the number/email iOS shows for an unknown sender. Null when
 * tug can't tell, and then the pop-up offers no reply box.
 */
/**
 * Whether a reply sent from a pop-up reads the conversation (seen here, read and cleared on the
 * phone), as replying on the phone would. Only a reply the phone took: a failed or unconfirmed one
 * leaves the conversation unread, with the notification still there to come back to. No stored
 * row (an older backend) keeps the old behaviour.
 */
export function replyReadsConversation(message: SmsMessage | null | undefined): boolean {
  return !message || message.status === "accepted" || message.status === "sent";
}

export function replyAddress(n: PhoneNotification, messages: SmsMessage[], contacts: Contact[]): string | null {
  if (n.appId !== MESSAGES_APP || !isConversation(n)) return null;
  const key = threadKey(n);
  const conversation = groupConversations([n], messages, contacts).find((c) => c.key === key);
  return conversation?.address ?? addressInTitle(n.title);
}

export function toastSpec(n: PhoneNotification, messages: SmsMessage[], contacts: Contact[]): ToastSpec {
  const conversation = isConversation(n);
  const ringing = n.category === "incomingCall";
  return {
    id: n.id,
    title: [appLabel(n), n.title].filter(Boolean).join(" · "),
    body: [n.subtitle, n.message].filter(Boolean).join("\n"),
    name: cleanName(n.title),
    replyTo: ringing ? null : replyAddress(n, messages, contacts),
    markRead: conversation && !ringing,
    code: findCode(n.message || n.subtitle)?.code ?? null,
    callBack: n.category === "missedCall" && n.live && n.removedAt == null && n.flags.positiveAction,
    clear: canClear(n),
  };
}
