// Connection health, as a pure function so Settings › iPhone › Connection can show each link's
// state with a plain-English reason/fix, and so the logic can be unit-tested without a phone.
// It never re-derives a fact another surface owns: the link's own state comes from
// lib/connectionStatus (same words as the sidebar), and notifications, texts and contacts from the
// switches in lib/phoneSwitches (same answer as the switches card). So the health rows can't say
// "Contacts: 214 synced" while the Sync Contacts switch says off.

import type { DeviceStatus } from "../types/protocol";
import { connectionSentence } from "./connectionStatus";
import { CHECKING_MAX_MS, phoneSwitches, type PhoneSwitch, type SwitchContext } from "./phoneSwitches";

/**
 * How a single link is doing. "waiting" = can't tell yet, with the reason; "off" = a switch to
 * flip; "unavailable" = the phone isn't offering it right now and tug keeps trying (no user fix).
 */
export type LinkState = "ok" | "off" | "error" | "waiting" | "unavailable";

export interface HealthLink {
  key: string;
  label: string;
  state: LinkState;
  /** One line: what's wrong and, where the owner can act, how to fix it. */
  detail: string;
}

/** Counts the status snapshot doesn't carry, fetched separately in the store. */
export interface HealthCounts {
  contacts: number;
  calls: number;
}

/**
 * Every link between tug and the iPhone, in the order they come up: the radio, this PC being
 * visible, then (once connected) notifications, media, battery, texts, contacts and calls.
 * `ctx` is the same context the switches card uses (`tug.switchContext`), so both bound
 * "checking" identically.
 */
export function connectionHealth(s: DeviceStatus, counts: HealthCounts, ctx?: SwitchContext): HealthLink[] {
  const connected = s.connection === "connected";
  const switches = phoneSwitches(s, ctx);
  const sw = (key: PhoneSwitch["key"]) => switches.find((x) => x.key === key)!;
  // Downstream links can only be judged once the phone is connected; say why, in the sidebar's words.
  const whenConnected = (): HealthLink | null =>
    connected ? null : { key: "", label: "", state: "waiting" as const, detail: connectionSentence(s) };
  // Media and battery are retried while linked; "waiting" for them is bounded like "Checking…".
  const settled = !!ctx && ctx.connectedSince != null && ctx.now - ctx.connectedSince >= CHECKING_MAX_MS;

  return [
    radioLink(s),
    visibleLink(s),
    fill("notifications", "Notifications", whenConnected(), () => notificationsLink(s, sw("notifications"))),
    fill("media", "Media", whenConnected(), () => mediaLink(s, settled)),
    fill("battery", "Battery", whenConnected(), () => batteryLink(s, settled)),
    fill("texts", "Texts", whenConnected(), () => textsLink(s, sw("messages"))),
    fill("contacts", "Contacts", whenConnected(), () => contactsLink(s, sw("contacts"), counts.contacts)),
    fill("calls", "Recent calls", whenConnected(), () => callsLink(sw("contacts"), counts.calls)),
  ];
}

/** Use the "waiting" placeholder when not connected, otherwise the link's own state. */
function fill(key: string, label: string, waiting: HealthLink | null, make: () => HealthLink): HealthLink {
  return waiting ? { ...waiting, key, label } : make();
}

function radioLink(s: DeviceStatus): HealthLink {
  const base = { key: "radio", label: "Bluetooth radio" };
  switch (s.radio) {
    case "on":
      return { ...base, state: "ok", detail: "Bluetooth is on." };
    case "off":
      return { ...base, state: "error", detail: "Bluetooth is off. Turn it on in Windows to reach your iPhone." };
    case "unavailable":
      return { ...base, state: "error", detail: "This PC has no usable Bluetooth adapter. A Bluetooth 5 USB adapter fixes this." };
    default:
      return { ...base, state: "waiting", detail: "Checking Bluetooth…" };
  }
}

function visibleLink(s: DeviceStatus): HealthLink {
  const base = { key: "advertising", label: "Visible to iPhone" };
  if (s.peripheralSupported === false) {
    return {
      ...base,
      state: "error",
      detail: "This PC's Bluetooth adapter can't act as a peripheral, so the iPhone can't connect to it. A Bluetooth 5 USB adapter fixes this.",
    };
  }
  switch (s.advertising) {
    case "on":
      return { ...base, state: "ok", detail: "Your iPhone can find and reconnect to this PC." };
    case "starting":
      return { ...base, state: "waiting", detail: "Starting…" };
    case "error":
      return { ...base, state: "error", detail: "Couldn't advertise to your iPhone. Turn Visible to iPhone off and on again." };
    default:
      return { ...base, state: "off", detail: "Turn on Visible to iPhone so your phone can reconnect." };
  }
}

function notificationsLink(s: DeviceStatus, x: PhoneSwitch): HealthLink {
  const base = { key: "notifications", label: "Notifications" };
  if (s.pairingStale) {
    return { ...base, state: "error", detail: "Your iPhone forgot this PC. Pair again from the setup steps below." };
  }
  if (x.state === "on") return { ...base, state: "ok", detail: "Your iPhone is sharing its notifications." };
  return { ...base, state: "off", detail: "Turn on Share System Notifications on your iPhone so its notifications reach tug." };
}

function mediaLink(s: DeviceStatus, settled: boolean): HealthLink {
  const base = { key: "media", label: "Media" };
  if (s.services.media) return { ...base, state: "ok", detail: "Now Playing and media controls are connected." };
  return settled
    ? { ...base, state: "unavailable", detail: "Your iPhone isn't sharing media controls on this connection. tug keeps trying." }
    : { ...base, state: "waiting", detail: "Waiting for your iPhone to share media controls." };
}

function batteryLink(s: DeviceStatus, settled: boolean): HealthLink {
  const base = { key: "battery", label: "Battery" };
  if (s.services.battery) {
    const level = s.battery != null ? ` Currently ${s.battery}%.` : "";
    return { ...base, state: "ok", detail: `Your iPhone's battery level is shared.${level}` };
  }
  return settled
    ? { ...base, state: "unavailable", detail: "Your iPhone isn't sharing its battery level on this connection. tug keeps trying." }
    : { ...base, state: "waiting", detail: "Waiting for your iPhone to share its battery level." };
}

function textsLink(s: DeviceStatus, x: PhoneSwitch): HealthLink {
  const base = { key: "texts", label: "Texts" };
  const phone = s.textsDevice ?? "your iPhone";
  if (s.textsPairing === "broken") {
    return {
      ...base,
      state: "error",
      detail: `Windows' texts pairing with ${phone} stopped working. In Bluetooth settings, remove ${phone} and add it again.`,
    };
  }
  if (s.textsPairing === "missing") {
    return { ...base, state: "off", detail: "Texts need a second pairing made from this PC. Set it up under Settings › iPhone." };
  }
  // The phone's own reason, already plain English (e.g. "turn on Show Notifications").
  if (x.state === "off") return { ...base, state: "off", detail: capitalise(s.messagesError ?? x.note) };
  if (x.state === "on") return { ...base, state: "ok", detail: "Reading and sending texts works." };
  if (x.state === "checking") return { ...base, state: "waiting", detail: "Waiting for message access to open." };
  return { ...base, state: "unavailable", detail: "Your iPhone hasn't opened message access yet. tug keeps trying." };
}

function contactsLink(s: DeviceStatus, x: PhoneSwitch, count: number): HealthLink {
  const base = { key: "contacts", label: "Contacts" };
  switch (x.state) {
    case "off":
      // The phone's own words for a refusal; otherwise the switch's line (which explains saved names).
      return { ...base, state: "off", detail: s.contactsError ? capitalise(s.contactsError) : x.note };
    case "on":
      return {
        ...base,
        state: "ok",
        detail: count > 0 ? `${count} ${count === 1 ? "contact" : "contacts"} synced, so names show instead of numbers.` : "Your iPhone is sharing its contacts.",
      };
    case "checking":
      return { ...base, state: "waiting", detail: "Checking Sync Contacts on your iPhone…" };
    default:
      return { ...base, state: "waiting", detail: x.note };
  }
}

function callsLink(contacts: PhoneSwitch, count: number): HealthLink {
  const base = { key: "calls", label: "Recent calls" };
  const loaded = `${count} recent ${count === 1 ? "call" : "calls"}`;
  // Recent calls come over PBAP behind the same Sync Contacts switch.
  switch (contacts.state) {
    case "off":
      return {
        ...base,
        state: "off",
        detail:
          count > 0
            ? `Recent calls need Sync Contacts on your iPhone. The ${loaded} here were loaded earlier.`
            : "Recent calls need Sync Contacts on your iPhone.",
      };
    case "on":
      return count > 0
        ? { ...base, state: "ok", detail: `${loaded} loaded from your iPhone.` }
        : { ...base, state: "ok", detail: "Your iPhone has no recent calls to share." };
    default:
      return { ...base, state: "waiting", detail: contacts.state === "checking" ? "Checking Sync Contacts on your iPhone…" : contacts.note };
  }
}

function capitalise(text: string): string {
  return text.length ? text[0].toUpperCase() + text.slice(1) : text;
}

/** How long ago the last error happened, as a short human string ("just now", "3m ago"). */
export function errorAge(atMs: number | null, nowMs: number): string | null {
  if (atMs == null) return null;
  const secs = Math.max(0, Math.round((nowMs - atMs) / 1000));
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}
