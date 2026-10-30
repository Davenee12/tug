// Who a new text can go to: the people you've texted lately, then your contacts, or a number typed
// in. Shared by New message (Ctrl+N) and the quick text composer, so both list the same people in
// the same order and find the same matches.
//
// Pure: callers hand in the conversations, contacts and who counts as known.

import type { Contact } from "../types/protocol";
import { normalizeAddress } from "./address";
import { cleanName, formatAddress, type Conversation } from "./format";

export interface Recipient {
  key: string;
  name: string;
  address: string;
  section: "Recent" | "Contacts" | "Number";
}

/** With nothing typed, this many of the people you text come first. */
export const RECENT_SHOWN = 6;

/** Case- and accent-insensitive: "jose" finds "José". */
export const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();

/**
 * People you've texted, most recent first (conversations come newest first), wherever tug knows
 * their number. Unknown senders (spam, short codes) stay out, like everywhere else Filter unknown
 * senders applies; a number can still be typed.
 */
export function recentRecipients(conversations: Conversation[], isKnown: (c: Conversation) => boolean): Recipient[] {
  return conversations
    .filter((c) => c.appId === "com.apple.MobileSMS" && c.address && isKnown(c))
    .map((c) => ({ key: `r:${c.address}`, name: c.contact, address: c.address!, section: "Recent" as const }));
}

/** Everyone in your contacts, A to Z. */
export function contactRecipients(contacts: Contact[]): Recipient[] {
  return [...contacts]
    .sort((a, b) => cleanName(a.name).localeCompare(cleanName(b.name)))
    .map((c) => ({ key: `c:${c.address}`, name: cleanName(c.name), address: c.address, section: "Contacts" as const }));
}

/**
 * What to offer for `query`: nothing typed lists the people you text, then everyone else; typing
 * narrows both by name or by 3+ digits of a number. A typed number nobody has can be texted too,
 * offered first. Each number appears once, under the first section that has it.
 */
export function recipientOptions(query: string, recent: Recipient[], contacts: Recipient[]): Recipient[] {
  const q = fold(query.trim());
  const digits = query.replace(/\D/g, "");
  const seen = new Set<string>();
  const take = (o: Recipient) => {
    if (seen.has(o.address)) return false;
    seen.add(o.address);
    return true;
  };
  if (!q) return [...recent.slice(0, RECENT_SHOWN).filter(take), ...contacts.filter(take)];
  const matches = (o: Recipient) => fold(o.name).includes(q) || (digits.length >= 3 && o.address.includes(digits));
  const out = [...recent.filter(matches).filter(take), ...contacts.filter(matches).filter(take)];
  // A typed number that isn't a saved contact can still be texted.
  if (digits.length >= 7 && !out.some((o) => o.address.endsWith(digits.slice(-10)))) {
    // +1 only for a bare 10-digit number on a PC in a +1 region; anything else goes as typed and
    // the iPhone reads it the way it would a number typed there.
    const address = normalizeAddress(query.trim().startsWith("+") ? `+${digits}` : digits);
    out.unshift({ key: `n:${address}`, name: formatAddress(address), address, section: "Number" });
  }
  return out;
}
