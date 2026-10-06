// Connection health, as a pure function so Settings › iPhone › Connection can show each link's
// state with a plain-English reason/fix, and so the logic can be unit-tested without a phone.
// Wording is kept in step with the inline iPhone checks in SettingsPage.vue and PhoneSetup.vue.

import type { DeviceStatus } from "../types/protocol";

/** How a single link is doing. "waiting" = can't tell yet (not connected); "off" = a switch to flip. */
export type LinkState = "ok" | "off" | "error" | "waiting";

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
 */
export function connectionHealth(s: DeviceStatus, counts: HealthCounts): HealthLink[] {
  const connected = s.connection === "connected";
  // Downstream links can only be judged once the phone is connected.
  const whenConnected = (): HealthLink | null =>
    connected
      ? null
      : {
          key: "",
          label: "",
          state: "waiting" as const,
          detail: s.awaitingUnlock
            ? "Your iPhone is connected but locked. Unlock it to reconnect."
            : s.device
              ? "Waiting for your iPhone to reconnect."
              : "Pair your iPhone to set this up.",
        };

  return [
    radioLink(s),
    visibleLink(s),
    fill("notifications", "Notifications", whenConnected(), () => notificationsLink(s)),
    fill("media", "Media", whenConnected(), () => mediaLink(s)),
    fill("battery", "Battery", whenConnected(), () => batteryLink(s)),
    fill("texts", "Texts", whenConnected(), () => textsLink(s)),
    fill("contacts", "Contacts", whenConnected(), () => contactsLink(s, counts.contacts)),
    fill("calls", "Recent calls", whenConnected(), () => callsLink(counts.calls)),
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

function notificationsLink(s: DeviceStatus): HealthLink {
  const base = { key: "notifications", label: "Notifications" };
  if (s.pairingStale) {
    return { ...base, state: "error", detail: "Your iPhone forgot this PC. Pair again from the setup steps below." };
  }
  if (s.services.notifications) {
    return { ...base, state: "ok", detail: "Your iPhone is sharing its notifications." };
  }
  return { ...base, state: "off", detail: "Turn on Share System Notifications on your iPhone so its notifications reach tug." };
}

function mediaLink(s: DeviceStatus): HealthLink {
  const base = { key: "media", label: "Media" };
  return s.services.media
    ? { ...base, state: "ok", detail: "Now Playing and media controls are connected." }
    : { ...base, state: "waiting", detail: "Waiting for your iPhone to share media controls." };
}

function batteryLink(s: DeviceStatus): HealthLink {
  const base = { key: "battery", label: "Battery" };
  if (s.services.battery) {
    const level = s.battery != null ? ` Currently ${s.battery}%.` : "";
    return { ...base, state: "ok", detail: `Your iPhone's battery level is shared.${level}` };
  }
  return { ...base, state: "waiting", detail: "Waiting for your iPhone to share its battery level." };
}

function textsLink(s: DeviceStatus): HealthLink {
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
  if (s.messagesError) {
    // The phone's own reason, already plain English (e.g. "turn on Show Notifications").
    return { ...base, state: "off", detail: capitalise(s.messagesError) };
  }
  if (s.services.messages) {
    return { ...base, state: "ok", detail: "Reading and sending texts works." };
  }
  return { ...base, state: "waiting", detail: "Waiting for message access to open." };
}

function contactsLink(s: DeviceStatus, count: number): HealthLink {
  const base = { key: "contacts", label: "Contacts" };
  if (s.contactsError) {
    return { ...base, state: "off", detail: capitalise(s.contactsError) };
  }
  if (count > 0) {
    return { ...base, state: "ok", detail: `${count} ${count === 1 ? "contact" : "contacts"} synced, so names show instead of numbers.` };
  }
  return { ...base, state: "off", detail: "Turn on Sync Contacts on your iPhone so tug shows names instead of numbers." };
}

function callsLink(count: number): HealthLink {
  const base = { key: "calls", label: "Recent calls" };
  return count > 0
    ? { ...base, state: "ok", detail: `${count} recent ${count === 1 ? "call" : "calls"} loaded from your iPhone.` }
    : { ...base, state: "waiting", detail: "No recent calls loaded yet." };
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
