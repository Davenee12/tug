// Spotting a messy pairing state so setup can offer "Start over" instead of looping. Dave's PC
// ended up with two iPhone bonds (an old "Old iPhone" and a new "Jordan's iPhone"), and
// once with a bond the phone had forgotten. These are pure decisions over the device list tug
// already has (each entry's `kind` comes from the Rust device_kind::classify), so they're
// unit-tested without a phone. We don't guess: a problem is only reported on clear evidence.

import type { DiscoveredDevice } from "../types/protocol";

/** What's wrong with the current pairing, if anything. */
export type PairingProblem = "duplicates" | "remembered-missing" | null;

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
