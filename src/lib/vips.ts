// "Always let through" people (VIPs), stored as normalised addresses picked from contacts. A text
// or call is from a VIP when its sender resolves to one of those addresses, or when the notification
// is titled with a VIP contact's name (iOS titles a known sender by name). Modelled on senders.ts.

import type { Contact } from "../types/protocol";
import { cleanName, nameKey } from "./format";
import { isAddressLike, normalizeAddress, numberTail } from "./address";

export interface VipIndex {
  addresses: Set<string>;
  tails: Set<string>;
  /** Lower-cased names of contacts whose address is a VIP, for matching a notification's title. */
  names: Set<string>;
}

/** Build the lookup once per change, rather than per notification. */
export function vipIndex(vips: Iterable<string>, contacts: Contact[]): VipIndex {
  const addresses = new Set<string>();
  const tails = new Set<string>();
  for (const raw of vips) {
    const a = normalizeAddress(raw);
    if (!a) continue;
    addresses.add(a);
    const t = numberTail(a);
    if (t) tails.add(t);
  }
  const names = new Set<string>();
  for (const c of contacts) {
    const a = normalizeAddress(c.address);
    const t = numberTail(a);
    if (addresses.has(a) || (t !== null && tails.has(t))) {
      const n = nameKey(c.name);
      if (n && !isAddressLike(n)) names.add(n);
    }
  }
  return { addresses, tails, names };
}

function matchesAddress(index: VipIndex, raw: string): boolean {
  const a = normalizeAddress(raw);
  if (!a) return false;
  if (index.addresses.has(a)) return true;
  const t = numberTail(a);
  return t !== null && index.tails.has(t);
}

/**
 * Whether this sender is a VIP. `name` is a notification title or a contact name (which iOS shows
 * for a known sender); `address` is the resolved number/email when tug has it. Either can be null.
 */
export function isVip(index: VipIndex, who: { name?: string | null; address?: string | null }): boolean {
  if (who.address && matchesAddress(index, who.address)) return true;
  const name = who.name ? cleanName(who.name) : "";
  if (!name) return false;
  // A title that's really a number/email (an unknown sender) is matched as an address, not a name.
  if (isAddressLike(name)) return matchesAddress(index, name);
  return index.names.has(nameKey(name));
}
