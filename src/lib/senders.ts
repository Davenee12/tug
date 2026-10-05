// Filter Unknown Senders, modelled on the iPhone's: texts from people you know are your
// conversations; texts from everyone else (unsaved numbers, short codes, emails, spam) wait in
// their own list, without a badge or a Windows pop-up. One-time codes still pop up.
//
// KNOWN, any of:
//   • the sender is a contact: their number matches a contact (however it's formatted), or iOS
//     titled the text with a name (it only does that for people in the iPhone's contacts; tug's
//     own copy of the contacts may lag behind, and old names are mapped by the backend's aliases);
//   • you've texted that number from tug (any outgoing message to it);
//   • you moved them to your conversations (the `ui.knownSenders` allowlist).
// UNKNOWN: everyone else. Only Messages (SMS/iMessage) is filtered; other chat apps are as before.

import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";
import { findCode } from "./codes";
import { MESSAGES_APP, cleanName, isConversation, type Conversation, type Thread } from "./format";
import { isAddressLike, normalizeAddress, numberTail } from "./address";

/** Everything a sender can be known by, built once per change instead of per sender. */
export interface SenderIndex {
  addresses: Set<string>;
  tails: Set<string>;
}

/**
 * A contact's name that is a real name. A "name" that is just a number or email was learned
 * from an unknown sender's notification (iOS titles those with the number), so it isn't a
 * saved contact. "Maybe: Jane" is iOS guessing from Mail, not a contact either.
 */
function isRealName(name: string): boolean {
  const n = cleanName(name);
  return n !== "" && !isAddressLike(n) && !/^maybe:/i.test(n);
}

export function senderIndex(contacts: Contact[], outgoingAddresses: Iterable<string>, allowlist: Iterable<string>): SenderIndex {
  const addresses = new Set<string>();
  const tails = new Set<string>();
  const add = (raw: string) => {
    const a = normalizeAddress(raw);
    if (!a) return;
    addresses.add(a);
    const tail = numberTail(a);
    if (tail) tails.add(tail);
  };
  for (const c of contacts) if (isRealName(c.name)) add(c.address);
  for (const a of outgoingAddresses) add(a);
  for (const a of allowlist) add(a);
  return { addresses, tails };
}

/** Every number you've sent a text to (pending and failed sends count: you meant to). */
export function outgoingAddresses(messages: SmsMessage[]): string[] {
  return [...new Set(messages.filter((m) => m.direction === "out").map((m) => m.address))];
}

/** `sender` is an address (from MAP) or a notification title (a name, or a number for strangers). */
export function isKnownTo(index: SenderIndex, sender: string): boolean {
  const s = cleanName(sender);
  if (!s) return false;
  if (!isAddressLike(s)) return isRealName(s);
  const a = normalizeAddress(s);
  if (index.addresses.has(a)) return true;
  const tail = numberTail(a);
  return tail !== null && index.tails.has(tail);
}

export function isKnownSender(
  sender: string,
  contacts: Contact[],
  outgoingAddresses: Iterable<string>,
  allowlist: Iterable<string>,
): boolean {
  return isKnownTo(senderIndex(contacts, outgoingAddresses, allowlist), sender);
}

/** A Messages conversation is known when its name or any of its numbers is. Other apps always are. */
export function isKnownConversation(c: Pick<Conversation, "appId" | "contact" | "addresses">, index: SenderIndex): boolean {
  if (c.appId !== MESSAGES_APP) return true;
  return isKnownTo(index, c.contact) || c.addresses.some((a) => isKnownTo(index, a));
}

/** Known conversations, and unknown senders' (each newest first, as given). */
export function splitConversations<C extends Pick<Conversation, "appId" | "contact" | "addresses">>(
  convs: C[],
  index: SenderIndex,
): { known: C[]; unknown: C[] } {
  const known: C[] = [];
  const unknown: C[] = [];
  for (const c of convs) (isKnownConversation(c, index) ? known : unknown).push(c);
  return { known, unknown };
}

/** A notification thread counts toward unread (badge, tray, taskbar dot) unless it's an unknown sender's. */
export function threadCounts(t: Pick<Thread, "appId" | "items">, index: SenderIndex): boolean {
  return t.appId !== MESSAGES_APP || t.items.some((n) => isKnownTo(index, n.title));
}

/**
 * May this notification pop up on Windows, as far as the sender goes? Yes unless it's a text
 * from an unknown sender, and even then yes when it carries a one-time code (codes mostly
 * come from short codes nobody saves).
 */
export function senderMayToast(n: PhoneNotification, index: SenderIndex): boolean {
  if (n.appId !== MESSAGES_APP || !isConversation(n) || isKnownTo(index, n.title)) return true;
  return findCode(n.message || n.subtitle) !== null;
}
