// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppName,
  CallRecord,
  Contact,
  SearchResults,
  SmsMessage,
  DeviceStatus,
  DiscoveredDevice,
  MediaCommand,
  NowPlaying,
  PairingRequest,
  PhoneNotification,
} from "../types/protocol";

export const api = {
  getStatus: () => invoke<DeviceStatus>("get_status"),
  getNowPlaying: () => invoke<NowPlaying>("get_now_playing"),
  listNotifications: (limit: number, beforeId?: number) =>
    invoke<PhoneNotification[]>("list_notifications", { limit, beforeId: beforeId ?? null }),
  searchNotifications: (query: string, limit: number) =>
    invoke<PhoneNotification[]>("search_notifications", { query, limit }),
  clearHistory: () => invoke<void>("clear_history"),
  performAction: (id: number, positive: boolean) => invoke<void>("perform_action", { id, positive }),
  mediaCommand: (command: MediaCommand) => invoke<void>("media_command", { command }),
  startDiscovery: () => invoke<void>("start_discovery"),
  stopDiscovery: () => invoke<void>("stop_discovery"),
  pairDevice: (id: string) => invoke<void>("pair_device", { id }),
  confirmPairing: (accept: boolean) => invoke<void>("confirm_pairing", { accept }),
  useDevice: (id: string) => invoke<void>("use_device", { id }),
  forgetDevice: () => invoke<void>("forget_device"),
  setAdvertising: (enabled: boolean) => invoke<void>("set_advertising", { enabled }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  listMessages: (limit: number) => invoke<SmsMessage[]>("list_messages", { limit }),
  getContacts: () => invoke<Contact[]>("get_contacts"),
  sendMessage: (address: string, text: string) => invoke<SmsMessage>("send_message", { address, text }),
  refreshMessages: () => invoke<void>("refresh_messages"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  setUnread: (count: number) => invoke<void>("set_unread", { count }),
  openWindowsSettings: (page: "bluetooth" | "location" | "notifications") => invoke<void>("open_windows_settings", { page }),
  /** Open an http(s) link (a notification's "Open in browser") in the default browser. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  setHidden: (notificationIds: number[], messageIds: number[], hidden: boolean) =>
    invoke<void>("set_hidden", { notificationIds, messageIds, hidden }),
  locate: () => invoke<{ latitude: number; longitude: number }>("locate"),
  setWatching: (on: boolean) => invoke<void>("set_watching", { on }),
  appIcon: (appId: string) => invoke<string | null>("app_icon", { appId }),
  placeLookup: (latitude: number, longitude: number) => invoke<string>("place_lookup", { latitude, longitude }),
  markRead: (messageIds: number[]) => invoke<void>("mark_read", { messageIds }),
  searchAll: (query: string, limit: number) => invoke<SearchResults>("search_all", { query, limit }),
  getCalls: () => invoke<CallRecord[]>("get_calls"),
  refreshCalls: () => invoke<void>("refresh_calls"),
  /** Experimental hands-free dialing; with no number, only checks the link can be opened. */
  dial: (number: string | null) => invoke<void>("dial", { number }),
};

interface EventPayloads {
  "device-status": DeviceStatus;
  "now-playing": NowPlaying;
  notification: PhoneNotification;
  "notification-removed": number;
  "app-name": AppName;
  "discovered-devices": DiscoveredDevice[];
  "pairing-request": PairingRequest;
  "pairing-request-closed": null;
  message: SmsMessage;
  contacts: Contact[];
  "open-latest-conversation": null;
  calls: CallRecord[];
}

export function on<E extends keyof EventPayloads>(
  event: E,
  handler: (payload: EventPayloads[E]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[E]>(event, (e) => handler(e.payload));
}

/** Tauri rejects with the Rust `Err(String)`; normalise anything else. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong";
}
