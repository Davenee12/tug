// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppName,
  BtInventory,
  CallRecord,
  Contact,
  SearchResults,
  SmsMessage,
  DeviceStatus,
  TugboatSkipped,
  TugboatStatus,
  TugboatTextArrived,
  DiscoveredDevice,
  MediaCommand,
  NowPlaying,
  PairingRequest,
  PhoneNotification,
  RepeatMode,
  SpotifyAlbumDetail,
  SpotifyArtist,
  SpotifyArtistDetail,
  SpotifyDevice,
  SpotifyKind,
  SpotifyPlayer,
  SpotifyPlaylist,
  SpotifyQueue,
  SpotifySearch,
  SpotifyStatus,
  SpotifyTrack,
  ToastPressed,
  ToastSpec,
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
  /** Re-run the iPhone inquiry without disturbing the LE watcher or rows already listed. */
  rescanDiscovery: () => invoke<void>("rescan_discovery"),
  /** Unpair a leftover phone's LE and Classic bonds, then keep scanning so it shows up fresh. */
  removePairing: (id: string) => invoke<void>("remove_pairing", { id }),
  pairDevice: (id: string) => invoke<void>("pair_device", { id }),
  confirmPairing: (accept: boolean) => invoke<void>("confirm_pairing", { accept }),
  useDevice: (id: string) => invoke<void>("use_device", { id }),
  forgetDevice: () => invoke<void>("forget_device"),
  /** Pair the iPhone's Classic (texts) side from inside tug; the PIN shows via pairing-request. */
  pairTexts: () => invoke<void>("pair_texts"),
  setAdvertising: (enabled: boolean) => invoke<void>("set_advertising", { enabled }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  /** Whether tug starts with Windows (reads the actual registry entry). */
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  listMessages: (limit: number) => invoke<SmsMessage[]>("list_messages", { limit }),
  getContacts: () => invoke<Contact[]>("get_contacts"),
  /** Resolves with the stored message even when the phone didn't take it (status "failed"); throws only when nothing was recorded. */
  sendMessage: (address: string, text: string) => invoke<SmsMessage>("send_message", { address, text }),
  /** Send a failed message again: the same message, to the same number. */
  retryMessage: (id: number) => invoke<SmsMessage>("retry_message", { id }),
  refreshMessages: () => invoke<void>("refresh_messages"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  /** Build the support report, copy it to the clipboard, and return it (for a preview/length). */
  copyDiagnostics: () => invoke<string>("copy_diagnostics"),
  /** The Bluetooth inventory: what the iPhone exposes to tug (runs it now; privacy-safe). */
  btInventory: () => invoke<BtInventory>("bt_inventory"),
  /** Open tug's log folder in Explorer. */
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  /** Forward an uncaught frontend error to the Rust log (name/message/stack-top only, no bodies). */
  logFrontendError: (kind: string, name: string, message: string, source: string) =>
    invoke<void>("log_frontend_error", { kind, name, message, source }),
  setUnread: (count: number) => invoke<void>("set_unread", { count }),
  popupsBlocked: () => invoke<boolean>("popups_blocked"),
  openWindowsSettings: (page: "bluetooth" | "location" | "notifications") => invoke<void>("open_windows_settings", { page }),
  /** Open an http(s) link (a notification's "Open in browser") in the default browser. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** Delete a conversation (all of it, by sender and address); returns the delete's stamp, which undoes it as `undoAt`. */
  setConversationHidden: (senders: [string, string][], addresses: string[], undoAt: number | null) =>
    invoke<number>("set_conversation_hidden", { senders, addresses, undoAt }),
  locate: () => invoke<{ latitude: number; longitude: number }>("locate"),
  setWatching: (on: boolean) => invoke<void>("set_watching", { on }),
  /** Check the iPhone's switches now (Sync Contacts is read by asking the phone). */
  checkSwitches: () => invoke<void>("check_switches"),
  appIcon: (appId: string) => invoke<string | null>("app_icon", { appId }),
  appWebsite: (appId: string) => invoke<string | null>("app_website", { appId }),
  contactPhoto: (key: string) => invoke<string | null>("contact_photo", { key }),
  placeLookup: (latitude: number, longitude: number) => invoke<string>("place_lookup", { latitude, longitude }),
  markRead: (messageIds: number[]) => invoke<void>("mark_read", { messageIds }),
  searchAll: (query: string, limit: number) => invoke<SearchResults>("search_all", { query, limit }),
  getCalls: () => invoke<CallRecord[]>("get_calls"),
  refreshCalls: () => invoke<void>("refresh_calls"),
  /** Experimental hands-free dialing; with no number, only checks the link can be opened. */
  dial: (number: string | null) => invoke<void>("dial", { number }),
  /** A Windows pop-up with buttons (falls back to a plain one in the backend). */
  showToast: (spec: ToastSpec) => invoke<void>("show_toast", { spec }),
  // --- Spotify connector ---
  spotifyStatus: () => invoke<SpotifyStatus>("spotify_status"),
  /** Opens the browser for OAuth; resolves once the loopback redirect comes back. */
  spotifyConnect: () => invoke<SpotifyStatus>("spotify_connect"),
  spotifyDisconnect: () => invoke<SpotifyStatus>("spotify_disconnect"),
  spotifyPlaylists: () => invoke<SpotifyPlaylist[]>("spotify_playlists"),
  spotifyCover: (url: string) => invoke<string | null>("spotify_cover", { url }),
  /** Play a playlist/album/artist context on a device (null = the iPhone, with the open-Spotify wait). */
  spotifyPlayContext: (uri: string, deviceId: string | null) =>
    invoke<void>("spotify_play_context", { uri, deviceId }),
  /** Play one track, optionally inside a context so the queue keeps going. */
  spotifyPlayTrack: (uri: string, contextUri: string | null, deviceId: string | null) =>
    invoke<void>("spotify_play_track", { uri, contextUri, deviceId }),
  spotifyPlayer: () => invoke<SpotifyPlayer | null>("spotify_player"),
  spotifySetRepeat: (mode: RepeatMode) => invoke<void>("spotify_set_repeat", { mode }),
  spotifySetShuffle: (on: boolean) => invoke<void>("spotify_set_shuffle", { on }),
  spotifySetSaved: (uri: string, saved: boolean) => invoke<void>("spotify_set_saved", { uri, saved }),
  spotifySearch: (query: string, kinds: SpotifyKind[], offset: number) =>
    invoke<SpotifySearch>("spotify_search", { query, kinds, offset }),
  spotifyQueue: () => invoke<SpotifyQueue>("spotify_queue"),
  spotifyAddToQueue: (uri: string) => invoke<void>("spotify_add_to_queue", { uri }),
  spotifyRecentlyPlayed: () => invoke<SpotifyTrack[]>("spotify_recently_played"),
  spotifyTopTracks: (timeRange: string) => invoke<SpotifyTrack[]>("spotify_top_tracks", { timeRange }),
  spotifyTopArtists: (timeRange: string) => invoke<SpotifyArtist[]>("spotify_top_artists", { timeRange }),
  spotifyDevices: () => invoke<SpotifyDevice[]>("spotify_devices"),
  spotifyTransfer: (deviceId: string) => invoke<void>("spotify_transfer", { deviceId }),
  spotifySeek: (positionMs: number) => invoke<void>("spotify_seek", { positionMs }),
  spotifyAlbum: (id: string) => invoke<SpotifyAlbumDetail>("spotify_album", { id }),
  spotifyArtist: (id: string) => invoke<SpotifyArtistDetail>("spotify_artist", { id }),
  spotifyPlaylistItems: (id: string) => invoke<SpotifyTrack[]>("spotify_playlist_items", { id }),
  spotifyAddToPlaylist: (playlistId: string, trackUri: string) =>
    invoke<void>("spotify_add_to_playlist", { playlistId, trackUri }),
  // --- Tugboat ---
  /** Open Tugboat (a fresh code), or get the session already open. */
  tugboatStart: () => invoke<TugboatStatus>("tugboat_start"),
  tugboatStop: () => invoke<void>("tugboat_stop"),
  tugboatStatus: () => invoke<TugboatStatus>("tugboat_status"),
  /** The QR link on the clipboard, kept out of clipboard history and sync (it carries the secret). */
  tugboatCopyLink: () => invoke<void>("tugboat_copy_link"),
  /** Windows' file picker, then offer what was picked. */
  tugboatPickFiles: () => invoke<TugboatSkipped[]>("tugboat_pick_files"),
  tugboatRemoveOffer: (id: string) => invoke<void>("tugboat_remove_offer", { id }),
  /** Text for the phone to copy (empty clears it). */
  tugboatSendText: (text: string) => invoke<void>("tugboat_send_text", { text }),
  /** Open the Tugboat folder, or select a file Tugboat saved. */
  tugboatOpenFolder: (path: string | null) => invoke<void>("tugboat_open_folder", { path }),
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
  "open-settings": null;
  calls: CallRecord[];
  "toast-pressed": ToastPressed;
  "tugboat-status": TugboatStatus;
  "tugboat-text": TugboatTextArrived;
  /** Files dropped onto tug's window were offered (from Rust): the ones skipped. */
  "tugboat-dropped": TugboatSkipped[];
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
