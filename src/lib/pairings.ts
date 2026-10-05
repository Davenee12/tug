// Spotting a messy pairing state so setup can offer "Start over" instead of looping. Dave's PC
// ended up with two iPhone bonds (an old "DTD iPhone Max 15 Pro" and a new "Dave's iPhone"), and
// once with a bond the phone had forgotten. These are pure decisions over the device list tug
// already has (each entry's `kind` comes from the Rust device_kind::classify), so they're
// unit-tested without a phone. We don't guess: a problem is only reported on clear evidence.

import type { DeviceStatus, DiscoveredDevice } from "../types/protocol";

/** What's wrong with the current pairing, if anything. */
export type PairingProblem = "duplicates" | "remembered-missing" | null;

/** The discovered devices split into what setup should offer. */
export interface DeviceLists {
  /** iPhones to offer with a primary Pair/Use action, likeliest first. */
  phones: DiscoveredDevice[];
  /** Unknown devices (which could be a nameless just-connected iPhone): a quiet fallback list. */
  others: DiscoveredDevice[];
  /** How many accessories (keyboards, headphones) were hidden entirely. */
  hiddenAccessories: number;
}

/**
 * Split the discovered Bluetooth devices into what the setup step should show. A phone is offered
 * up front whenever Windows classifies it as one (named "… iPhone", or by LE Appearance / Classic
 * Class of Device) — it does NOT have to be connected to this PC first, which was the old LightBlue
 * assumption that left the wizard's list empty.
 *
 * Both transports are considered: a freshly-forgotten iPhone sitting on Settings › Bluetooth is
 * only discoverable over Classic (its LE adverts are anonymous and nameless), so Classic phones
 * must be offered too. When one phone shows on both transports, the LE entry wins and the Classic
 * duplicate is dropped, so the user sees one row. Pure, so the split is unit-tested without a phone.
 */
export function setupDeviceLists(discovered: DiscoveredDevice[]): DeviceLists {
  // Connected first, then by name, so a phone actively linked to the PC sits at the top.
  const order = (a: DiscoveredDevice, b: DiscoveredDevice) =>
    Number(b.connected) - Number(a.connected) || a.name.localeCompare(b.name);

  // One row per phone. An iPhone shows up on both transports with the same name; an unpaired
  // iPhone only accepts pairing over Classic (LE pairing fails with status 19 on Dave's iPhone,
  // and the LE bond then comes along via cross-transport keys). So when both are seen, the
  // Classic row wins unless the LE side is already paired or connected (the LightBlue path).
  const classicNames = new Set(
    discovered
      .filter((d) => d.transport === "classic" && d.kind === "phone")
      .map(realName)
      .filter((n): n is string => n !== null),
  );
  const leUsable = (d: DiscoveredDevice) => {
    const n = realName(d);
    return d.paired || d.connected || n === null || !classicNames.has(n);
  };
  const lePhones = discovered.filter((d) => d.transport === "le" && d.kind === "phone" && leUsable(d));
  const leNames = new Set(lePhones.map(realName).filter((n): n is string => n !== null));
  // Classic phones, minus any that are the same phone already offered over LE.
  const classicPhones = discovered.filter((d) => {
    if (d.transport !== "classic" || d.kind !== "phone") return false;
    const n = realName(d);
    return !(n !== null && leNames.has(n));
  });

  return {
    phones: [...lePhones, ...classicPhones].sort(order),
    // Unknowns (a nameless just-connected iPhone) stay LE-only: a nameless Classic device with no
    // phone signal is an accessory, not a maybe-iPhone, so it shouldn't masquerade as one. An LE
    // unknown named like a Classic phone is that phone's other side, already offered above.
    others: discovered.filter((d) => d.transport === "le" && d.kind === "unknown" && leUsable(d)).sort(order),
    hiddenAccessories: discovered.filter((d) => d.kind === "accessory").length,
  };
}

/** Lower-cased, trimmed name, or null when there isn't a real one yet ("Unnamed device"). */
function realName(d: DiscoveredDevice): string | null {
  const n = d.name.trim().toLowerCase();
  return n && n !== "unnamed device" ? n : null;
}

/** Paired devices Windows classifies as a phone (either Bluetooth transport). */
export function pairedPhones(discovered: DiscoveredDevice[]): DiscoveredDevice[] {
  return discovered.filter((d) => d.paired && d.kind === "phone");
}

/**
 * Whether more than one distinct iPhone is paired in Windows. The LE and Classic sides of one
 * phone share a name, so we count distinct names rather than entries; an old bond the user
 * replaced carries a different name and tips this over.
 */
export function hasDuplicateIphones(discovered: DiscoveredDevice[]): boolean {
  const names = new Set<string>();
  for (const d of pairedPhones(discovered)) {
    const n = realName(d);
    if (n) names.add(n);
  }
  return names.size >= 2;
}

/**
 * The pairing problem to surface, if any, given the devices tug can see and the id it remembers
 * as the notifications phone.
 *
 * - "remembered-missing": a paired iPhone is present but none of them is the one tug remembers —
 *   the phone was replaced (or its bond removed) while a different one stayed paired.
 * - "duplicates": more than one iPhone is paired and none is clearly "ours".
 *
 * Nothing is reported until at least one paired iPhone is actually in the list, so an empty scan
 * (or a phone simply out of range) never trips it.
 */
export function pairingProblem(discovered: DiscoveredDevice[], rememberedId: string | null): PairingProblem {
  const phones = pairedPhones(discovered);
  if (phones.length === 0) return null;
  if (rememberedId && !discovered.some((d) => d.id === rememberedId)) return "remembered-missing";
  if (hasDuplicateIphones(discovered)) return "duplicates";
  return null;
}

/**
 * A device went from unpaired to paired without tug starting the pairing — Windows' own system
 * dialog did it (phone-initiated, e.g. the user tapped this PC in the iPhone's Bluetooth list).
 * An unpackaged app can't intercept that, so the code shows in Windows, not in tug; the UI uses
 * this to point the user at Windows' prompt instead of leaving them hunting for a code.
 */
export function startedOutsideTug(
  wasPaired: boolean | undefined,
  nowPaired: boolean,
  tugInitiated: boolean,
): boolean {
  return wasPaired === false && nowPaired && !tugInitiated;
}

/**
 * Whether the iPhone looks to have forgotten this PC while Windows still holds the bond — Dave
 * forgot the PC on the phone but both bonds stayed, so LightBlue kept dropping, LE connects
 * failed with stale-bond errors and MAP CONNECT was refused 0xC3.
 *
 * - "forgotten": the LE link reported the stale-bond HRESULT (pairingStale) — a definite signal.
 * - "maybe": the bond exists but the phone won't let the link connect and isn't sharing
 *   notifications, with a message refusal (0xC3) on top. That 0xC3 alone is ambiguous (it's also
 *   how "Show Notifications off" looks), so this is worded tentatively and covers both causes.
 */
export type BondHint = "forgotten" | "maybe" | null;

export function bondHint(s: DeviceStatus): BondHint {
  if (s.pairingStale) return "forgotten";
  if (s.device && s.connection !== "connected" && !s.services.notifications && !!s.messagesError) {
    return "maybe";
  }
  return null;
}
