// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppName,
  AttachmentPreview,
  BackupImported,
  BackupInfo,
  BackupStatus,
  BtInventory,
  CallRecord,
  CodeFilled,
  CodeHotkeyStatus,
  Contact,
  SearchResults,
  ShareState,
  FocusState,
  SmsMessage,
  DeviceStatus,
  DevToolsConfirm,
  DevToolsStatus,
  TugboatClipEvent,
  TugboatSkipped,
  TugboatStatus,
  TugboatTextArrived,
  DiscoveredDevice,
  GamePad,
  GameId,
  MediaCommand,
  NowPlaying,
  PairingRequest,
  PcAudioStatus,
  PhoneNotification,
  QuickAction,
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
  UpdateApplyAnswer,
  UpdateStatus,
  VoiceEngine,
  VoiceModelId,
  VoiceStatus,
  VoiceTyped,
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
  // --- Play iPhone audio on this PC ---
  pcAudioStatus: () => invoke<PcAudioStatus>("pc_audio_status"),
  /** Play on this PC (true) or Stop. Turning on answers "connecting"; the outcome comes as `pc-audio`. */
  pcAudioSet: (on: boolean) => invoke<PcAudioStatus>("pc_audio_set", { on }),
  /** "Turn on automatically when my iPhone connects", for the phone tug uses now. */
  pcAudioSetAuto: (on: boolean) => invoke<PcAudioStatus>("pc_audio_set_auto", { on }),
  // --- Pause music and videos on this PC while the iPhone rings (lib/callPause) ---
  /** Pause what's playing on this PC; answers how many things tug holds paused. */
  pcMediaPause: () => invoke<number>("pc_media_pause"),
  /** Play again what tug paused and nobody changed since; answers how many played. */
  pcMediaResume: () => invoke<number>("pc_media_resume"),
  /** Let go of what tug paused, leaving it paused. */
  pcMediaRelease: () => invoke<void>("pc_media_release"),
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
  /** Code fill: a verification code (digits only) onto the clipboard, out of history and sync, cleared after 2 minutes. */
  copyCodePrivately: (code: string) => invoke<void>("copy_code", { code }),
  codeHotkeyStatus: () => invoke<CodeHotkeyStatus>("code_hotkey_status"),
  /** Taskbar quick actions waiting to run, oldest first; taking them clears them (each runs once). */
  takeQuickActions: () => invoke<QuickAction[]>("take_quick_actions"),
  /** Register the type-the-code shortcut (or unregister it); says if another app holds it. */
  setCodeHotkey: (enabled: boolean, keys: string) => invoke<CodeHotkeyStatus>("set_code_hotkey", { enabled, keys }),
  quickTextHotkeyStatus: () => invoke<CodeHotkeyStatus>("quick_text_hotkey_status"),
  /** Register the quick text shortcut (or unregister it); says if another app holds it. */
  setQuickTextHotkey: (enabled: boolean, keys: string) =>
    invoke<CodeHotkeyStatus>("set_quick_text_hotkey", { enabled, keys }),
  /** The quick text composer closed: with `giveBack`, the app the shortcut was pressed in gets the keyboard back. Sends nothing. */
  quickTextDone: (giveBack: boolean) => invoke<void>("quick_text_done", { giveBack }),
  /** Voice typing's state; with `languages`, the speech languages installed on this PC too (Settings). */
  voiceStatus: (languages: boolean) => invoke<VoiceStatus>("voice_status", { languages }),
  /** Turn voice typing on (shortcut, language) or off, and save it; says if the shortcut is taken or no language is installed. */
  setVoiceTyping: (enabled: boolean, keys: string, language: string | null) =>
    invoke<VoiceStatus>("set_voice_typing", { enabled, keys, language }),
  /** Pick voice typing's engine and save it; a whisper.cpp engine's model starts downloading if it isn't here. */
  setVoiceEngine: (engine: VoiceEngine) => invoke<VoiceStatus>("set_voice_engine", { engine }),
  /** Download (or resume) a speech model; progress comes as `voice-status` events. */
  voiceModelDownload: (model: VoiceModelId) => invoke<VoiceStatus>("voice_model_download", { model }),
  /** Cancel a speech model download (what it had is deleted, and it isn't started again by itself). */
  voiceModelCancel: (model: VoiceModelId) => invoke<VoiceStatus>("voice_model_cancel", { model }),
  /** Delete every downloaded speech model (and paused downloads); Basic listens until one is downloaded again. */
  voiceModelsDelete: () => invoke<VoiceStatus>("voice_models_delete"),
  /** The language Most accurate listens in ("auto" or a code from voiceSpokenLanguages.json). */
  setVoiceSpokenLanguage: (language: string) => invoke<VoiceStatus>("set_voice_spoken_language", { language }),
  /** Whether contacts' names help the whisper.cpp engines spell them. */
  setVoiceContactHints: (on: boolean) => invoke<VoiceStatus>("set_voice_contact_hints", { on }),
  /** The personal dictionary: words and names voice typing should spell the person's way. */
  voiceDictionary: () => invoke<string[]>("voice_dictionary"),
  /** Save the personal dictionary; answers with it as saved (tidied, without duplicates). */
  setVoiceDictionary: (words: string[]) => invoke<string[]>("set_voice_dictionary", { words }),
  /** The "Get voice typing ready now?" card has been shown: it never shows by itself again. */
  voiceReadyPromptShown: () => invoke<VoiceStatus>("voice_ready_prompt_shown"),
  /** Build the support report, copy it to the clipboard, and return it (for a preview/length). */
  copyDiagnostics: () => invoke<string>("copy_diagnostics"),
  /** Settings › About › Report a problem: GitHub's bug report form, filled in, in the browser. */
  reportProblem: (unclean: boolean) => invoke<void>("report_problem", { unclean }),
  /** This version's last run didn't close normally (once per version; null after). */
  uncleanExit: () => invoke<string | null>("unclean_exit"),
  /** The Bluetooth inventory: what the iPhone exposes to tug (runs it now; privacy-safe). */
  btInventory: () => invoke<BtInventory>("bt_inventory"),
  /** Open tug's log folder in Explorer. */
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  /** Forward an uncaught frontend error to the Rust log (name/message/stack-top only, no bodies). */
  logFrontendError: (kind: string, name: string, message: string, source: string) =>
    invoke<void>("log_frontend_error", { kind, name, message, source }),
  setUnread: (count: number) => invoke<void>("set_unread", { count }),
  popupsBlocked: () => invoke<boolean>("popups_blocked"),
  openWindowsSettings: (page: "bluetooth" | "location" | "notifications" | "microphone" | "language") => invoke<void>("open_windows_settings", { page }),
  /** Open an http(s) link (a notification's "Open in browser") in the default browser. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /**
   * Delete a conversation (all of it, by sender and address);
   * returns the delete's stamp, which undoes it as `undoAt`.
   */
  setConversationHidden: (senders: [string, string][], addresses: string[], undoAt: number | null) =>
    invoke<number>("set_conversation_hidden", { senders, addresses, undoAt }),
  locate: () => invoke<{ latitude: number; longitude: number }>("locate"),
  setWatching: (on: boolean) => invoke<void>("set_watching", { on }),
  /** Check the iPhone's switches now (Sync Contacts is read by asking the phone). */
  checkSwitches: () => invoke<void>("check_switches"),
  setSwitchesVisible: (on: boolean) => invoke<void>("set_switches_visible", { on }),
  appIcon: (appId: string) => invoke<string | null>("app_icon", { appId }),
  appWebsite: (appId: string) => invoke<string | null>("app_website", { appId }),
  /** The album's cover (data URI) from the cache or Apple's catalog; null when there's none or Show album art is off. */
  albumArt: (artist: string, album: string) => invoke<string | null>("album_art", { artist, album }),
  contactPhoto: (key: string) => invoke<string | null>("contact_photo", { key }),
  placeLookup: (latitude: number, longitude: number) => invoke<string>("place_lookup", { latitude, longitude }),
  markRead: (messageIds: number[]) => invoke<void>("mark_read", { messageIds }),
  warmTexts: () => invoke<void>("warm_texts"),
  searchAll: (query: string, limit: number) => invoke<SearchResults>("search_all", { query, limit }),
  getCalls: () => invoke<CallRecord[]>("get_calls"),
  refreshCalls: () => invoke<void>("refresh_calls"),
  /** Experimental hands-free dialing; with no number, only checks the link can be opened. */
  dial: (number: string | null) => invoke<void>("dial", { number }),
  /** A Windows pop-up with buttons (falls back to a plain one in the backend). */
  showToast: (spec: ToastSpec) => invoke<void>("show_toast", { spec }),
  /** The window is on screen (or not): while it is, the backend checks for screen sharing every 2 s. Returns the answer now. */
  screenShareWatch: (on: boolean) => invoke<ShareState>("screen_share_watch", { on }),
  focusState: () => invoke<FocusState>("focus_state"),
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
  /** Whether this Windows account is an administrator (the firewall prompt may need one). */
  tugboatUserIsAdmin: () => invoke<boolean>("tugboat_user_is_admin"),
  /** The QR link on the clipboard, kept out of clipboard history and sync (it carries the secret). */
  tugboatCopyLink: () => invoke<void>("tugboat_copy_link"),
  /** Windows' file picker, then offer what was picked. */
  tugboatPickFiles: () => invoke<TugboatSkipped[]>("tugboat_pick_files"),
  tugboatRemoveOffer: (id: string) => invoke<void>("tugboat_remove_offer", { id }),
  /** Text for the phone to copy (empty clears it). */
  tugboatSendText: (text: string) => invoke<void>("tugboat_send_text", { text }),
  /** "Send clipboard to iPhone": the clipboard as it is now, held for the phone for up to 2 minutes. */
  tugboatSendClipboard: () => invoke<TugboatStatus>("tugboat_send_clipboard"),
  /** Open the Tugboat folder, or select a file Tugboat saved. */
  tugboatOpenFolder: (path: string | null) => invoke<void>("tugboat_open_folder", { path }),
  // --- The games' phone controller ---
  /** Open Tugboat if needed and the controller channel for `game`, for one phone or two (a
   * two-player game); asking again changes how many. Tugboat's status carries the QR code. */
  gamePadOpen: (players: 1 | 2 = 1, game: GameId = "tugboat-run") => invoke<TugboatStatus>("game_pad_open", { players, game }),
  /** The game closed: the phone's inputs are refused again. */
  gamePadClose: () => invoke<void>("game_pad_close"),
  /** What a phone's controller shows: paused, hits this run (it buzzes on a new one), and in a race
   * which player it is (its colour). */
  gamePadFeedback: (seat: 1 | 2, paused: boolean, hits: number, player: 1 | 2 | null) =>
    invoke<void>("game_pad_feedback", { seat, paused, hits, player }),
  // --- Developer tools ---
  devtoolsStatus: () => invoke<DevToolsStatus>("devtools_status"),
  /** "Let AI tools use tug". */
  devtoolsSetEnabled: (enabled: boolean) => invoke<DevToolsStatus>("devtools_set_enabled", { enabled }),
  devtoolsSetPermission: (key: string, on: boolean) => invoke<DevToolsStatus>("devtools_set_permission", { key, on }),
  /** Settings › Developer tools › "Send tug text without asking". */
  devtoolsSetTextsWithoutAsking: (on: boolean) => invoke<DevToolsStatus>("devtools_set_texts_without_asking", { on }),
  /** A new token: connected AI tools must be restarted. */
  devtoolsRevoke: () => invoke<DevToolsStatus>("devtools_revoke"),
  /** The confirmation card's Send (true) or Don't send. */
  devtoolsConfirm: (id: number, send: boolean) => invoke<void>("devtools_confirm", { id, send }),
  /** Add tug's command folder to (or remove it from) the user's PATH. */
  devtoolsSetOnPath: (on: boolean) => invoke<DevToolsStatus>("devtools_set_on_path", { on }),
  // --- Automatic updates (src-tauri/src/updater) ---
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Check for updates now; answers Checking, the outcome comes as `update-status`. */
  updateCheck: () => invoke<UpdateStatus>("update_check"),
  /** Download the release a check found (automatic downloads off). */
  updateDownload: () => invoke<UpdateStatus>("update_download"),
  /** "Download updates automatically". */
  updateSetAuto: (on: boolean) => invoke<UpdateStatus>("update_set_auto", { on }),
  /** Restart now: installs and restarts (this never answers then), or says what has to finish first. */
  updateApply: (resume: string | null) => invoke<UpdateApplyAnswer>("update_apply", { resume }),
  /** The new release's page on GitHub (its notes; the portable download). */
  updateOpenReleasePage: () => invoke<void>("update_open_release_page"),
  /** The screen to come back to after an update restart, once. */
  updateTakeResume: () => invoke<string | null>("update_take_resume"),
  // --- Full message history (an iPhone backup) ---
  /** The newest text of every conversation, so ones older than the recent page still list. */
  conversationHeads: (limit: number) => invoke<SmsMessage[]>("conversation_heads", { limit }),
  /** One conversation's whole history, by its numbers. */
  conversationMessages: (addresses: string[]) => invoke<SmsMessage[]>("conversation_messages", { addresses }),
  attachmentPreview: (id: number) => invoke<AttachmentPreview>("attachment_preview", { id }),
  /** Open a stored photo or video in its default app (usually Photos). */
  openAttachment: (id: number) => invoke<void>("open_attachment", { id }),
  backupList: () => invoke<BackupInfo[]>("backup_list"),
  /** Windows' folder picker; null when nothing was picked. */
  backupPickFolder: () => invoke<BackupInfo | null>("backup_pick_folder"),
  backupStatus: () => invoke<BackupStatus>("backup_status"),
  /** Unlock (with the password for an encrypted backup, used once and never kept) and read it. */
  backupOpen: (id: string, password: string | null) => invoke<void>("backup_open", { id, password }),
  backupImport: (texts: boolean, photos: boolean) => invoke<void>("backup_import", { texts, photos }),
  /** Cancel reading or importing, or close a backup that was read. */
  backupCancel: () => invoke<void>("backup_cancel"),
  backupImported: () => invoke<BackupImported>("backup_imported"),
  /** Remove every text the import added and every photo it copied; resolves with how many texts. */
  backupRemoveImported: () => invoke<number>("backup_remove_imported"),
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
  /** The type-the-code shortcut was pressed (from Rust, `code_fill`): what happened, never the code. */
  "code-filled": CodeFilled;
  /** A taskbar jump-list task was clicked (from Rust, `quick_actions`): take what's waiting. */
  "quick-action": null;
  /** Previews started or stopped being hidden because the screen is shared or presented (`share_privacy/`). */
  "screen-share": ShareState;
  "focus-state": FocusState;
  "tugboat-status": TugboatStatus;
  "tugboat-text": TugboatTextArrived;
  /** Files dropped onto tug's window were offered (from Rust): the ones skipped. */
  "tugboat-dropped": TugboatSkipped[];
  /** The PC's clipboard reached the phone, or no phone took it within 2 minutes. */
  "tugboat-clip": TugboatClipEvent;
  /** The tray's "Send clipboard to iPhone" (the window sends it, like Ctrl+K). */
  "tugboat-send-clipboard": null;
  /** The phone controller changed (Tugboat Run only). */
  "game-pad": GamePad;
  "devtools-status": DevToolsStatus;
  "pc-audio": PcAudioStatus;
  "voice-status": VoiceStatus;
  "voice-typed": VoiceTyped;
  /** Automatic updates changed (`updater/`). */
  "update-status": UpdateStatus;
  /** A text an AI tool (or `tug text`) wants to send; null once it's answered or gone. */
  "devtools-confirm": DevToolsConfirm | null;
  /** Bringing in history from an iPhone backup: progress, the scan, the outcome (`backup/`). */
  "backup-import": BackupStatus;
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
