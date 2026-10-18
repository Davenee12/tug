// Mirrors the serde types in src-tauri/src/{state,store,ams,ancs,map/calls,toast}.rs.
// Keep field names and enum strings in sync with the Rust side.

export type RadioState = "unknown" | "on" | "off" | "unavailable";
export type AdvertisingState = "off" | "starting" | "on" | "error";
export type ConnectionState = "noDevice" | "disconnected" | "connecting" | "connected";

export interface PairedDevice {
  id: string;
  name: string;
  /** Apple's model identifier ("iPhone16,2") read over Bluetooth; null until the phone reports it.
   *  `lib/phoneModel` turns it into a name and picture. */
  model: string | null;
}

export interface Services {
  notifications: boolean;
  media: boolean;
  battery: boolean;
  /** Bluetooth MAP session (read inbox, send replies) is up. */
  messages: boolean;
}

export interface DeviceStatus {
  /**
   * Counts the backend's status changes. Events and the startup read can arrive out of order, so
   * the store keeps the highest (`lib/ordering`). Always sent; optional only so the store's
   * initial status and test fixtures needn't name it.
   */
  rev?: number;
  radio: RadioState;
  peripheralSupported: boolean | null;
  advertising: AdvertisingState;
  device: PairedDevice | null;
  connection: ConnectionState;
  battery: number | null;
  services: Services;
  lastError: string | null;
  /** Unix ms when `lastError` last changed to its current value (for "2m ago"). */
  lastErrorAt: number | null;
  /** The iPhone rejects this PC's notifications pairing (forgotten on the phone): pair again. */
  pairingStale: boolean;
  /** iOS is holding the ANCS subscribe open until "Allow" is tapped on the phone: prompt for it. */
  awaitingPhoneAllow: boolean;
  /** The iPhone is connected but locked (no ANCS yet): tell the user to unlock it to reconnect. */
  awaitingUnlock: boolean;
  /**
   * The paired phone has no notification service and doesn't look like an Apple device (an
   * Android phone): tug says it works with iPhone instead of "Unlock your iPhone". Optional only
   * so the store's initial status needn't name it.
   */
  notIphone?: boolean;
  /** With awaitingUnlock: the phone restarted after working this run (unlock and tug reconnects). */
  phoneRestarted?: boolean;
  /**
   * Message access has worked with this phone since it was set up (or tug started), so texts
   * being down now is a reconnect, not a setup problem. Optional only so the store's initial
   * status needn't name it.
   */
  textsWereConnected?: boolean;
  /** Message access has failed several times in a row ("Can't reach your texts right now"). */
  textsUnreachable?: boolean;
  /** tug is rebuilding the link on its own (Bluetooth stalled, the PC woke): say "Reconnecting…", not "Waiting". */
  reconnecting: boolean;
  /** The iPhone is away (out of range): steady until it connects again, not flipping with each retry. */
  away?: boolean;
  /** Why message access isn't available, when the user can fix it. */
  messagesError: string | null;
  /** Why the phone's contacts aren't available, when the user can fix it. */
  contactsError: string | null;
  /** The phone shared contacts on the current connection (Sync Contacts on). */
  contactsShared: boolean;
  /** The phone keeps sharing an empty phonebook (or refused): Sync Contacts is off. Never true with contactsShared. */
  contactsOff: boolean;
  /** Whether the texts (Classic) pairing works, is missing, or needs making again. */
  textsPairing: TextsPairing;
  /** The phone Windows has paired for texts (what to remove when it needs re-pairing). */
  textsDevice: string | null;
  /** Live texts (MAP notifications): off, starting, active, or fell back to polling. */
  liveTexts: LiveTexts;
}

export type TextsPairing = "unknown" | "missing" | "broken" | "ok";

export type LiveTexts = "off" | "starting" | "active" | "unavailable";

export type Category =
  | "other"
  | "incomingCall"
  | "missedCall"
  | "voicemail"
  | "social"
  | "schedule"
  | "email"
  | "news"
  | "healthAndFitness"
  | "businessAndFinance"
  | "location"
  | "entertainment";

export interface EventFlags {
  silent: boolean;
  important: boolean;
  preExisting: boolean;
  positiveAction: boolean;
  negativeAction: boolean;
}

export interface PhoneNotification {
  id: number;
  appId: string;
  appName: string | null;
  category: Category;
  title: string;
  subtitle: string;
  message: string;
  /** iPhone-local time, ISO without zone. */
  postedAt: string | null;
  /** Unix ms when tug received it. */
  receivedAt: number;
  flags: EventFlags;
  positiveLabel: string;
  negativeLabel: string;
  removedAt: number | null;
  /** Still on the phone in the current connection, so actions can be sent. */
  live: boolean;
  /**
   * The event that carried this inserted it: tug had never stored it before. False on updates and
   * on every list/search read (iOS flags replays after a reconnect as pre-existing, including what
   * arrived during the gap; this tells the gap's new ones apart).
   */
  fresh: boolean;
}

export type PlaybackState = "unknown" | "paused" | "playing" | "rewinding" | "fastForwarding";

export type MediaCommand =
  | "play"
  | "pause"
  | "togglePlayPause"
  | "nextTrack"
  | "previousTrack"
  | "volumeUp"
  | "volumeDown"
  | "advanceRepeatMode"
  | "skipForward"
  | "skipBackward"
  | "likeTrack"
  | "dislikeTrack";

export interface NowPlaying {
  /** Counts Now Playing changes (`Stamped` in state.rs), like `DeviceStatus.rev`. Optional only for fixtures. */
  rev?: number;
  player: string | null;
  state: PlaybackState;
  rate: number | null;
  elapsed: number | null;
  /** Unix ms when the phone reported `elapsed`; progress advances from here. */
  elapsedAt: number | null;
  volume: number | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  duration: number | null;
  /** The player's repeat mode, when it reports one. */
  repeat: RepeatMode | null;
  available: MediaCommand[];
}

export type RepeatMode = "off" | "one" | "all";

export type Transport = "le" | "classic";

export interface DiscoveredDevice {
  id: string;
  name: string;
  transport: Transport;
  paired: boolean;
  connected: boolean;
  canPair: boolean;
  /** "accessory": keyboards, mice, headphones — never offered as the iPhone. */
  kind: DeviceKind;
}

export type DeviceKind = "phone" | "accessory" | "unknown";

export interface PairingRequest {
  deviceName: string;
  pin: string | null;
  /** ConfirmOnly: Windows accepts on its own and the user taps Pair on the iPhone — tug just waits. */
  confirmOnPhone: boolean;
}

export interface AppName {
  appId: string;
  appName: string;
}

/** A conversation message from a connector (first: the iPhone over Bluetooth MAP). */
export interface SmsMessage {
  id: number;
  source: string;
  direction: "in" | "out";
  /** Normalised phone number or email. */
  address: string;
  contactName: string | null;
  body: string;
  /** ISO time when the phone reported one: phone-local without a zone, or UTC with Z. */
  sentAt: string | null;
  receivedAt: number;
  /**
   * Outgoing: pending → accepted (taken by the iPhone; not proof of delivery) → sent (a MAP
   * SendingSuccess event confirmed it left), or failed. Without live texts a send stops at
   * accepted; the UI shows both accepted and sent as "Sent".
   * Unconfirmed: the whole text went out but the phone never answered, so it may have sent.
   */
  status: "received" | "pending" | "accepted" | "sent" | "failed" | "unconfirmed";
  /**
   * The MAP message type the phone reported (SMS_GSM, SMS_CDMA, MMS, EMAIL, IM), for telling
   * iMessage (IM) from a plain text. Null for history from before tug stored it, and for sends.
   */
  msgType: string | null;
  /**
   * Texts from before this one may be missing: it came in a catch-up where every text the phone
   * listed was new, so older ones from while tug was away may only be on the iPhone.
   */
  gapBefore: boolean;
  /** Edited on the phone after it was sent (from a backup). */
  edited?: boolean;
  /** Unsent on the phone: the words are gone (from a backup). */
  unsent?: boolean;
  /** Photos, videos and other attachments a backup had for it (absent means none). */
  attachments?: MessageAttachment[];
}

/** One attachment of a text, from an iPhone backup (`messages.rs` `StoredAttachment`). */
export interface MessageAttachment {
  id: number;
  kind: "photo" | "gif" | "sticker" | "video" | "audio" | "contact" | "location" | "file";
  /** The file is on this PC (photos and videos); otherwise only its kind is known. */
  stored: boolean;
  bytes: number | null;
}

/** A stored photo's preview (`backup/files.rs`). */
export interface AttachmentPreview {
  /** A JPEG data URI, when there's a picture to show. */
  dataUri: string | null;
  /** A HEIC photo Windows can't open without HEIF Image Extensions. */
  needsHeif: boolean;
  /** A HEIC photo HEIF Image Extensions opened but can't show without HEVC Video Extensions. */
  needsHevc: boolean;
}

/** One iPhone backup on this PC (`backup/discover.rs`). */
export interface BackupInfo {
  /** Opaque: names the backup back to tug, which keeps the folder itself. */
  id: string;
  deviceName: string;
  /** Apple's model identifier ("iPhone15,2"). */
  model: string | null;
  iosVersion: string | null;
  /** When it was last made (ms since 1970). */
  lastBackup: number | null;
  sizeBytes: number;
  encrypted: boolean;
  problem: "tooOld" | "incomplete" | "unreadable" | null;
}

/** What an import would add (`backup/import.rs` `Scan`). */
export interface BackupScan {
  texts: number;
  already: number;
  conversations: number;
  photos: number;
  videos: number;
  mediaBytes: number;
  /** New texts that carry photos (what "photos only" brings in). */
  photoTexts: number;
  /** Texts in group chats, which aren't brought in. */
  groupTexts: number;
}

/** What an import did (`backup/import.rs` `Summary`). */
export interface BackupSummary {
  texts: number;
  /** Pictures (photos, GIFs, stickers) copied to this PC. */
  photos: number;
  videos: number;
  conversations: number;
  already: number;
  /** Photos the backup listed but didn't have the file for. */
  missing: number;
  /** Texts in group chats, left out. */
  groupTexts: number;
}

export type BackupPhase = "idle" | "unlocking" | "reading" | "ready" | "importing" | "done" | "cancelled" | "failed";
export type BackupProblem =
  | "needsPassword"
  | "wrongPassword"
  | "tooOld"
  | "incomplete"
  | "unreadable"
  | "noMessages"
  | "cantWrite"
  | "failed";

/** The import's status (`backup/mod.rs` `BackupStatus`), sent as `backup-import` events. */
export interface BackupStatus {
  rev: number;
  phase: BackupPhase;
  backup: BackupInfo | null;
  /** 0–1 through the current step, when it can be measured. */
  progress: number | null;
  stage: "texts" | "photos" | null;
  done: number;
  total: number;
  scan: BackupScan | null;
  summary: BackupSummary | null;
  problem: BackupProblem | null;
}

/** What's in tug from a backup now (`backup/import.rs` `Imported`). */
export interface BackupImported {
  texts: number;
  photos: number;
  /** When the backup was made (ms since 1970). */
  backupDate: number | null;
}

export interface Contact {
  address: string;
  name: string;
}

export type CallDirection = "incoming" | "outgoing" | "missed";

/** One call from the phone's Recents (PBAP call history, `map/calls.rs`). */
export interface CallRecord {
  direction: CallDirection;
  /** The name the phone showed, if the caller is a contact. */
  name: string | null;
  /** Normalised like message addresses; null for a withheld number. */
  number: string | null;
  /** ISO time: phone-local without a zone, or UTC with `Z`. */
  at: string | null;
}

/** Universal search results (`search_all`). */
export interface SearchResults {
  people: Contact[];
  messages: SmsMessage[];
  notifications: PhoneNotification[];
}

/**
 * A quiet-hours schedule: during it, Windows pop-ups are held (notifications still collect in the
 * Feed, like Do not disturb). VIPs and calls still come through (see `shouldPopUp`).
 */
export interface QuietHours {
  enabled: boolean;
  /** 24-hour "HH:MM" local time. */
  start: string;
  end: string;
  /** Days the schedule runs, 0 = Sunday … 6 = Saturday. Empty means every day. */
  days: number[];
}

/** UI preferences persisted in SQLite under the `ui.` prefix. */
export interface UiSettings {
  toasts: boolean;
  doNotDisturb: boolean;
  mutedApps: string[];
  /** A schedule during which Windows pop-ups are held (like Do not disturb). */
  quietHours: QuietHours;
  /** People whose texts and calls pop up even during quiet hours or Do not disturb (normalised addresses). */
  vips: string[];
  /** Hold incoming-call pop-ups too (off by default: calls ring through quiet hours and DND). */
  muteCalls: boolean;
  /** Closing the window keeps tug running in the tray (read by the backend too). */
  closeToTray: boolean;
  /** Real app icons in the Feed, fetched once per app from Apple's App Store. */
  appIcons: boolean;
  /**
   * Album art on the Now Playing card from Apple's public catalog when Spotify has none (on by
   * default; read by the backend, `album_art.rs`). Sends only the artist and album name.
   */
  albumArt: boolean;
  /** A Windows pop-up when the iPhone's battery drops to 20% and 10%. */
  lowBattery: boolean;
  /** tug's pop-ups play the Windows sound (read by the backend, `toast/mod.rs`). */
  popupSound: boolean;
  /** Experimental Call buttons. Only a successful hands-free check (Settings › iPhone) turns them on. */
  dialing: boolean;
  /** Texts from unknown senders go to their own list in Messages: no badge, no pop-up (codes still pop up). */
  filterUnknown: boolean;
  /** Numbers/emails moved to conversations by hand ("Move to conversations"), normalised. */
  knownSenders: string[];
  /** Code fill: a newly arrived verification code goes on the clipboard (privately, cleared after 2 min). On by default. */
  autoCopyCodes: boolean;
  /** Code fill: a global shortcut types the latest code (read by the backend too, `code_fill/`). On by default. */
  typeCodeHotkey: boolean;
  /** Which shortcut, one of `src/lib/codeHotkeys.json` (read by the backend too). */
  typeCodeKeys: string;
  /** The sidebar's Play button and the Ctrl+K "Tugboat Run" command (on by default). */
  showGames: boolean;
  /** One-tap suggested replies on text pop-ups and in Messages (on by default; lib/smartReplies). */
  smartReplies: boolean;
  /** The suggestions for texts no rule matches (at most 3; lib/smartReplies `DEFAULT_REPLIES` unless changed). */
  smartReplyDefaults: string[];
  /** While the screen is shared or presented, pop-ups and the window hide texts and names (on by default; read by the backend, `share_privacy/`). */
  hidePreviewsWhileSharing: boolean;
}

/**
 * A Windows pop-up for one phone notification and what it offers (`show_toast`,
 * `src-tauri/src/toast/xml.rs`). The frontend decides whether it shows and what applies.
 */
export interface ToastSpec {
  /** The notification's id: what every button acts on. */
  id: number;
  title: string;
  body: string;
  /** Who it's from, for "Reply to …" and "Sent to …". */
  name: string;
  /** A text from a person tug can answer over message access: the reply box and Send. */
  replyTo: string | null;
  /** A text whose sender's name has several numbers: "Open in tug" (pick the number there) instead of a reply box. */
  open?: boolean;
  /** A conversation: "Mark read". */
  markRead: boolean;
  /** A one-time code: "Copy code". */
  code: string | null;
  /** A missed call still on the phone with its Dial action: "Call back". */
  callBack: boolean;
  /** Clearable on the phone (never a ringing call): "Clear". */
  clear: boolean;
  /**
   * Suggested replies, one button each, sent like a typed reply to `replyTo` (only with a reply
   * box). At most 3, each at most 40 characters; the backend fits them in beside the other buttons.
   */
  replies?: string[];
  /** The app it's from ("Messages"): all the pop-up says while previews are hidden. */
  app: string;
  /** What it is, for the wording while previews are hidden ("New message", "Incoming call"). */
  kind: ToastKind;
}

/** What a pop-up is about (`ToastKind` in `toast/xml.rs`); `info` is tug's own words, never hidden. */
export type ToastKind = "notification" | "message" | "call" | "missedCall" | "info";

/** Why previews are hidden (`share_privacy::Reason`); `unknown` when tug couldn't check, so it hides to be safe. */
export type ShareReason = "sharing" | "presenting" | "fullScreen" | "unknown";

/** Whether previews are hidden right now because the screen is shared or presented (`screen-share`, `share_privacy/`). */
export interface ShareState {
  active: boolean;
  reason: ShareReason | null;
}

/** A pop-up's body or button was pressed and carried out (`toast-pressed`, `toast/mod.rs`). */
export interface ToastPressed {
  kind: "open" | "read" | "replied" | "copied" | "calledBack";
  id: number;
  /** A reply: the text as stored (same row as its `message` events); null for other presses. */
  message: SmsMessage | null;
}

// --- Code fill (mirrors src-tauri/src/code_fill/mod.rs) ---

/** What a press of the type-the-code shortcut did (`code-filled`). The code itself is never sent. */
export interface CodeFilled {
  /** typed; noCode (none in the last 10 minutes); noTarget (tug's own window, or nothing, in front);
   *  keysHeld (Ctrl, Alt, Shift or Windows still down after the wait, so nothing was typed); failed. */
  outcome: "typed" | "noCode" | "noTarget" | "keysHeld" | "failed";
  /** Who the typed code came from; null unless typed. */
  from: string | null;
}

/**
 * A task from tug's taskbar jump list (`quick_actions::QuickAction`, serialized kebab-case: the
 * same id the task passes as `--action`), taken with `take_quick_actions`.
 */
export type QuickAction = "new-message" | "tugboat" | "copy-code" | "game" | "settings";

/** The type-the-code shortcut (`code_hotkey_status`, `set_code_hotkey`). */
export interface CodeHotkeyStatus {
  /** off (switched off, or not on this PC); on (registered); inUse (another app holds it). */
  state: "off" | "on" | "inUse";
  /** The shortcut it's about, e.g. "ctrl+shift+v". */
  keys: string;
}

// --- Spotify connector (mirrors src-tauri/src/spotify/{mod,model}.rs) ---

/** Spotify connection state for Settings. */
export interface SpotifyStatus {
  connected: boolean;
  /** The connected account's display name, when connected. */
  account: string | null;
  /**
   * Connected before tug asked for a scope it now needs (Your top, Recent, Add to playlist), so
   * those need a reconnect. Optional only so the store's initial value needn't name it.
   */
  needsReconnect?: boolean;
}

/** One of the user's own or followed playlists. */
export interface SpotifyPlaylist {
  /** `spotify:playlist:…`, played as the `context_uri`. */
  uri: string;
  /** Bare id, for reading songs and adding to the playlist. */
  id: string;
  name: string;
  owner: string | null;
  /** Null when Spotify doesn't say (playlists the user doesn't own). */
  trackCount: number | null;
  /** Small cover image URL; shown via `spotifyCover`, never loaded by the webview directly. */
  imageUrl: string | null;
  /** The user owns or collaborates on it: tug can open its songs and add to it. */
  owned: boolean;
}

/** A track as the panel lists it (search, queue, recently played, top, album and playlist songs). */
export interface SpotifyTrack {
  uri: string;
  name: string;
  /** All artists, joined "A, B". */
  artists: string;
  /** The primary artist's `spotify:artist:…`, for "open artist" from a track. */
  artistUri: string | null;
  album: string;
  /** The album's `spotify:album:…`, so a track can play in its album context. */
  albumUri: string | null;
  imageUrl: string | null;
  durationMs: number;
}

/** An album as search results and artist pages list it. */
export interface SpotifyAlbum {
  uri: string;
  id: string;
  name: string;
  artists: string;
  imageUrl: string | null;
  totalTracks: number | null;
  year: string | null;
}

/** An artist as search results list it and the artist page heads with. */
export interface SpotifyArtist {
  uri: string;
  id: string;
  name: string;
  imageUrl: string | null;
}

/** Whether another page exists per type, so the panel shows "Show more". */
export interface SpotifySearchMore {
  tracks: boolean;
  albums: boolean;
  artists: boolean;
  playlists: boolean;
}

/** A page of Spotify search results across the requested types. */
export interface SpotifySearch {
  tracks: SpotifyTrack[];
  albums: SpotifyAlbum[];
  artists: SpotifyArtist[];
  playlists: SpotifyPlaylist[];
  more: SpotifySearchMore;
}

/** The current playback queue: what's playing and what's next. */
export interface SpotifyQueue {
  currentlyPlaying: SpotifyTrack | null;
  queue: SpotifyTrack[];
}

/** A device playback can be sent to, for the "Play on" picker. */
export interface SpotifyDevice {
  id: string;
  name: string;
  /** Spotify's device type: "Smartphone", "Computer", "Speaker"… */
  kind: string;
  isActive: boolean;
}

/** An album page: the album, plus its songs. */
export interface SpotifyAlbumDetail {
  album: SpotifyAlbum;
  tracks: SpotifyTrack[];
}

/** An artist page: the artist, plus their albums. */
export interface SpotifyArtistDetail {
  artist: SpotifyArtist;
  albums: SpotifyAlbum[];
}

/** Which search type kinds the backend accepts. */
export type SpotifyKind = "track" | "album" | "artist" | "playlist";

/** The Spotify playback snapshot that augments Now Playing (repeat/shuffle/like/art). */
export interface SpotifyPlayer {
  isPlaying: boolean;
  shuffle: boolean;
  repeat: RepeatMode | null;
  /** Whether the current track is in the library (for the Like button); null if unknown. */
  saved: boolean | null;
  /** Album art as a `data:` URI, when available. */
  albumArt: string | null;
  /** The playing track's URI (what Like saves/removes). */
  trackUri: string | null;
  /** The playing track's name, checked against the phone's title before Like is offered. */
  trackName: string | null;
  /** The playing track's artists, "A, B" (the phone hides them on Spotify Connect). */
  trackArtists: string | null;
  /** The device Spotify says it is playing on (`/me/player` `device`). */
  deviceId: string | null;
  deviceName: string | null;
  deviceKind: string | null;
}

// --- Tugboat (src-tauri/src/tugboat/mod.rs, session.rs, qr.rs) ---

/** Off; showing the QR code; a phone has connected; or no network a phone could use. */
export type TugboatPhase = "off" | "waiting" | "connected" | "noNetwork";

/** The QR code as one SVG path in module units (the panel adds the quiet zone). */
export interface TugboatQr {
  size: number;
  path: string;
}

/** A file coming from the phone (in progress, or saved this session). */
export interface TugboatIncoming {
  id: string;
  name: string;
  size: number;
  received: number;
  done: boolean;
  /** Full path once saved, for "Show in folder". */
  path: string | null;
  at: number;
}

/** A file offered to the phone. */
export interface TugboatOffer {
  id: string;
  name: string;
  size: number;
  /** Times the phone fetched the whole file. */
  downloads: number;
}

/** Text the phone sent (it also went on the clipboard). */
export interface TugboatText {
  id: number;
  text: string;
  at: number;
}

/** Everything the Tugboat panel shows; also the "tugboat-status" event. */
export interface TugboatStatus {
  /**
   * Numbers each copy in the order the backend composed it; the store keeps the newest
   * (`lib/ordering`). Always sent; optional for fixtures and the store's own Off placeholder.
   */
  rev?: number;
  phase: TugboatPhase;
  /** The QR link (with the secret), also shown as the manual fallback. */
  url: string | null;
  /** "192.168.1.20:53211". */
  address: string | null;
  qr: TugboatQr | null;
  /**
   * The Player 2 code's link: the guest secret (derived from the session's, never the session's
   * own) after `#g.`. Only while a two-player game asks for a second phone; it can only steer.
   */
  guestUrl: string | null;
  guestQr: TugboatQr | null;
  /** "iPhone", once one has connected. */
  phone: string | null;
  /** The phone's page is open and checking in. */
  phoneActive: boolean;
  /** Where received files go (Pictures\Tugboat, or Downloads\Tugboat when Windows refused Pictures). */
  folder: string | null;
  /**
   * Windows Security (Controlled folder access) refused Pictures: files go to Downloads instead
   * ("fallback"), or can't be saved until tug is allowed ("blocked").
   */
  folderBlock: "fallback" | "blocked" | null;
  incoming: TugboatIncoming[];
  outgoing: TugboatOffer[];
  texts: TugboatText[];
  /** Text currently offered to the phone. */
  sentText: string | null;
  /** The phone is downloading a file from the PC right now. */
  sending: boolean;
  /** A file from the phone is arriving right now (an unfinished one may have been abandoned). */
  receiving: boolean;
  /** Why Tugboat turned itself off. */
  ended: "idle" | "hidden" | null;
  /** The last clipboard sent to the phone: its kind and whether the phone has it, never its contents. */
  clip: TugboatClip | null;
  /** Files `tug boat` (or an AI tool) is holding for the phone: names and when the hold ends. */
  held: TugboatHeld | null;
}

/** Files held for the phone after Send on tug's card (`tugboat/held.rs` `HeldStatus`). */
export interface TugboatHeld {
  /** What the phone sees them called. */
  names: string[];
  /** When the soonest hold ends (Unix ms). */
  expiresAt: number;
}

/** "tugboat-text": text from the phone went (or failed to go) onto the PC clipboard. */
export interface TugboatTextArrived {
  ok: boolean;
}

// --- Tugboat's shared clipboard (src-tauri/src/tugboat/clip.rs) ---

export type TugboatClipKind = "text" | "image";
/** Waiting for the phone; the phone has it; or no phone took it within 2 minutes (dropped). */
export type TugboatClipState = "waiting" | "delivered" | "expired";

/** The last clipboard sent with "Send clipboard to iPhone" (`ClipStatus`). */
export interface TugboatClip {
  id: number;
  kind: TugboatClipKind;
  state: TugboatClipState;
  /** When a waiting clipboard is dropped (Unix ms). */
  expiresAt: number;
}

/** "tugboat-clip": the clipboard reached the phone, or was dropped (`ClipEvent`). */
export interface TugboatClipEvent {
  kind: TugboatClipKind;
  state: TugboatClipState;
}

/** Which of tug's games has the phone controller (`tugboat/pad.rs` `GameId`). */
export type GameId = "tugboat-run" | "harbor-stack";

/**
 * The phone as a game's controller (`game-pad` event, `tugboat/pad.rs` `PadEvent`). Sent when
 * steering, boost or drop changes, and when the phone arrives or goes quiet.
 */
export interface GamePad {
  /** Which phone: 1 is the one that opened Tugboat, 2 a second phone (two-player games only). */
  seat: 1 | 2;
  connected: boolean;
  /** -1 (full left) to 1 (full right); 0 whenever not connected. Harbor Stack: the crane nudge. */
  steer: number;
  /** Tugboat Run's Boost (always false in Harbor Stack). */
  boost: boolean;
  /** Harbor Stack's Drop (always false in Tugboat Run). */
  drop: boolean;
}

/** A file that couldn't be offered to the phone, and why. */
export interface TugboatSkipped {
  name: string;
  reason: "tooBig" | "folder" | "unreadable" | "tooMany";
}

// --- Bluetooth inventory (src-tauri/src/bt_inventory) ---
// A diagnostic of what the iPhone exposes to tug. Privacy-safe by construction: UUIDs, property
// flags, field names, counts, enums, lengths and harmless values only.

/** One part of the inventory: what was found, or why it couldn't be. */
export type Probe<T> = { ok: T } | { unavailable: string };

export type InventoryTrigger = "connect" | "followUp" | "onDemand";

export interface BtGattCharacteristic {
  /** `0x2A19` for SIG UUIDs, the full 128-bit form otherwise. */
  uuid: string;
  name: string | null;
  properties: string[];
  /** For readable characteristics: "N bytes", an error, or why it wasn't read. Never the value. */
  read: string | null;
}

export interface BtGattService {
  uuid: string;
  name: string | null;
  characteristics: Probe<BtGattCharacteristic[]>;
}

export interface BtPnpId {
  vendorIdSource: string;
  vendorId: string;
  vendor: string | null;
  productId: string;
  productVersion: string;
}

export interface BtDeviceInformation {
  manufacturer: string | null;
  modelNumber: string | null;
  serialNumberLength: number | null;
  hardwareRevision: string | null;
  firmwareRevision: string | null;
  softwareRevision: string | null;
  systemIdLength: number | null;
  regulatoryDataLength: number | null;
  pnpId: BtPnpId | null;
  unreadable: Record<string, string>;
}

export interface BtCurrentTime {
  /** The phone's local wall-clock time, `YYYY-MM-DDTHH:MM:SS`. */
  local: string;
  dayOfWeek: string | null;
  fractions256: number;
  adjustReasons: string[];
}

export interface BtLocalTimeInfo {
  timeZoneMinutes: number | null;
  dstOffsetMinutes: number | null;
}

export interface BtReferenceTimeInfo {
  source: string;
  accuracyEighths: number | null;
  accuracy: string;
  daysSinceUpdate: number;
  hoursSinceUpdate: number;
}

export interface BtCurrentTimeReport {
  currentTime: Probe<BtCurrentTime>;
  currentTimeNotifies: boolean;
  localTimeInfo: Probe<BtLocalTimeInfo>;
  referenceTimeInfo: Probe<BtReferenceTimeInfo>;
  utcOffsetMinutes: number | null;
  /** Phone clock minus PC clock in seconds (positive: the phone is ahead). */
  skewSeconds: number | null;
}

export interface BtBatteryReport {
  characteristics: string[];
  level: number | null;
  levelNotifies: boolean;
  powerStateCharacteristic: boolean;
}

/** One AMS attribute read: a harmless value, presence + length, empty, or an error. */
export type BtAmsValue = { value: string } | { present: number } | "empty" | { error: string };

export interface BtAmsReport {
  attributes: { name: string; result: BtAmsValue }[];
  supportedCommands: string[] | null;
}

export interface BtAncsTally {
  added: number;
  modified: number;
  removed: number;
  categories: Record<string, number>;
  flags: Record<string, number>;
  detailsFetched: number;
  attributesPresent: Record<string, number>;
  messageSize: string;
  actionLabels: string[];
}

export interface BtVcardFieldCounts {
  contacts: number;
  /** Standard vCard property names and `X-` names; anything else is counted as "custom". */
  fields: Record<string, number>;
  /** Standard TEL types (and Apple's IPHONE/MAIN/OTHER); anything else is "custom". */
  telTypes: Record<string, number>;
}

export interface BtPbapReport {
  phonebooks: Record<string, { response: string; size: number | null }>;
  /** Why the phonebook size check didn't run (or failed) in this report. */
  phonebooksNote: string | null;
  /** From tug's own contacts pulls only ("contacts", "contacts+photos"). */
  fieldCounts: Record<string, BtVcardFieldCounts>;
  /** What fieldCounts covers: only the fields tug's own pulls ask for. */
  fieldCountsScope: string;
  probedAt: number | null;
}

export interface BtMapReport {
  folders: Probe<string[]>;
  listingTypes: Record<string, number>;
  mnsEvents: Record<string, number>;
  sdpFeatures: string[] | null;
  sdpMessageTypes: string | null;
}

export interface BtSdpRecord {
  serviceClasses: string[];
  profiles: string[];
  protocols: string[];
  supportedFeatures: string | null;
  featureNames: string[];
  details: Record<string, string>;
  attributeIds: string[];
}

export interface BtLinkReport {
  parameters: Probe<{ intervalMs: number; latency: number; supervisionTimeoutMs: number }>;
  phy: Probe<{ transmit: string; receive: string }>;
  maxPduSize: number | null;
}

export interface BtInventory {
  schema: number;
  trigger: InventoryTrigger;
  generatedAt: number;
  gatt: Probe<BtGattService[]>;
  deviceInformation: Probe<BtDeviceInformation>;
  currentTime: Probe<BtCurrentTimeReport>;
  battery: Probe<BtBatteryReport>;
  ams: Probe<BtAmsReport>;
  ancs: BtAncsTally;
  pbap: Probe<BtPbapReport>;
  map: Probe<BtMapReport>;
  classicSdp: Probe<BtSdpRecord[]>;
  link: Probe<BtLinkReport>;
  audioPlayback: Probe<{ candidates: number; iphoneListed: boolean }>;
}

// ---- Developer tools (src-tauri/src/devtools) ----

export type DevToolsPermissionKey =
  | "codes"
  | "search"
  | "dev_notifications"
  | "tugboat_files"
  | "phone_status"
  | "media"
  | "send_text"
  | "tugboat_send";

/** One switch in Settings › Developer tools (`PermissionState`). */
export interface DevToolsPermission {
  key: DevToolsPermissionKey;
  label: string;
  on: boolean;
}

/** A tool that has used tug (`ClientRecord`). `name` is what it called itself. */
export interface DevToolsClient {
  name: string;
  kind: "mcp" | "cli";
  /** Unix ms. */
  lastUsed: number;
}

/** What the "send this?" card has in common (`ConfirmRequest`). */
interface DevToolsConfirmBase {
  id: number;
  /** Who's asking, as it named itself. */
  tool: string;
  /** Unix ms after which it counts as not sent. */
  expiresAt: number;
}

/** "<tool> wants to text <name>" (`Ask::Text`). */
export interface DevToolsConfirmText extends DevToolsConfirmBase {
  kind: "text";
  toName: string;
  toAddress: string;
  message: string;
}

/** One file on the card (`ConfirmFile`): the name the phone will see, its size, and its folder. */
export interface DevToolsConfirmFile {
  name: string;
  size: number;
  /** null for what was piped into `tug boat -`. */
  folder: string | null;
}

/** "Send report.pdf (2.1 MB) to your iPhone?" (`Ask::Files`). */
export interface DevToolsConfirmFiles extends DevToolsConfirmBase {
  kind: "files";
  files: DevToolsConfirmFile[];
}

/** The "send this?" card: a text, or files for the phone. */
export type DevToolsConfirm = DevToolsConfirmText | DevToolsConfirmFiles;

/** Everything Settings › Developer tools shows (`DevToolsStatus`). */
export interface DevToolsStatus {
  /** Numbers each copy in the order the backend composed it; the store keeps the newest. Optional for fixtures. */
  rev?: number;
  enabled: boolean;
  permissions: DevToolsPermission[];
  clients: DevToolsClient[];
  /** Full path of the tug command (`...\bin\tug.exe`), or null if it isn't installed. */
  cliPath: string | null;
  cliDir: string | null;
  onPath: boolean;
  bridgeRunning: boolean;
  pending: DevToolsConfirm | null;
}

// ---- Play iPhone audio on this PC (`pc_audio/policy.rs`) ----

/** Whether the iPhone's audio is playing on this PC (`PcAudioState`). */
export type PcAudioState = "off" | "connecting" | "on";

/** Why it's off when it was wanted on (`PcAudioProblem`). */
export type PcAudioProblem = "notFound" | "denied" | "timedOut" | "failed" | "dropped";

/** Everything the PC audio button and Settings row show (`PcAudioStatus`). */
export interface PcAudioStatus {
  /**
   * Counts the changes: a command's answer and the event from the attempt can arrive in either
   * order, so the store keeps the highest (`lib/ordering`). Optional only for fixtures.
   */
  rev?: number;
  /** This version of Windows can do it (Windows 10 version 2004 and later). */
  supported: boolean;
  state: PcAudioState;
  problem: PcAudioProblem | null;
  /** "Turn on automatically when my iPhone connects", for the phone tug uses now. */
  auto: boolean;
}

// ---- Voice typing (`voice/mod.rs`) ----

/** A speech language installed on this PC (`VoiceLanguage`). */
export interface VoiceLanguage {
  /** Windows' language id in hex ("409" is English, United States). */
  id: string;
  /** "English (US)". */
  name: string;
}

/** off; on (shortcut registered); inUse (another app holds it); unavailable (nothing can listen: no Windows speech language, and the accurate engine can't stand in). */
export type VoiceState = "off" | "on" | "inUse" | "unavailable";

/** Voice typing's engine (`voice::engine::EngineChoice`): accurate (whisper.cpp with the English model), mostAccurate (whisper.cpp with the multilingual large-v3-turbo model) or basic (Windows' own recognizer). */
export type VoiceEngine = "accurate" | "mostAccurate" | "basic";

/** One of the downloadable speech models (`voice::model::ModelId`), named for the engine that runs it. */
export type VoiceModelId = "accurate" | "mostAccurate";

/** Where a speech model is (`voice::model::ModelState`). */
export type VoiceModelState = "missing" | "downloading" | "ready";

/** A whisper.cpp engine's speech model (`voice::model::ModelStatus`). */
export interface VoiceModel {
  state: VoiceModelState;
  /** Bytes downloaded so far (a paused download counts). */
  received: number;
  /** The model's size in bytes. */
  total: number;
  /** What went wrong with the last download, or with the model, for the person. */
  error: string | null;
}

/** Voice typing for Settings (`voice_status`, `set_voice_typing`, the `voice-status` event). */
export interface VoiceStatus {
  enabled: boolean;
  state: VoiceState;
  /** The hold-to-talk shortcut, one of `src/lib/voiceHotkeys.json`. */
  keys: string;
  /** The language listened in (an id from `languages`), null when none is installed. */
  language: string | null;
  /** Empty until Settings asks for them. */
  languages: VoiceLanguage[];
  /** Windows' privacy settings keep desktop apps from the microphone. */
  micBlocked: boolean;
  /** Turning it on failed, so it was switched back off: "Voice typing couldn't start: …". */
  startError: string | null;
  /** The engine picked in Settings. */
  engine: VoiceEngine;
  /** The engine a hold would use right now; null when nothing can listen. */
  activeEngine: VoiceEngine | null;
  /** Accurate's speech model. */
  model: VoiceModel;
  /** Most accurate's speech model. */
  mostAccurateModel: VoiceModel;
  /** The language Most accurate listens in: "auto" (detected) or a code from `src/lib/voiceSpokenLanguages.json`. */
  spokenLanguage: string;
  /** Contacts' names help the whisper.cpp engines spell them (on this PC only). */
  contactHints: boolean;
  /** This processor can run the whisper.cpp engines (AVX2). */
  accurateSupported: boolean;
  /** Offer the one-time "Get voice typing ready now?" card: the first launch after an install or update, voice typing never set up, never offered before. */
  readyPrompt: boolean;
  /** The default microphone is a phone-call (Bluetooth hands-free) headset. */
  phoneMic: boolean;
  /** Why the whisper.cpp engine picked isn't listening although it could: it failed to start on this PC (Basic listens until it's switched on again), there was too little free memory on the last hold (Basic listened), or too little for Most accurate (Accurate listened). */
  accurateProblem: "failed" | "lowMemory" | "lowMemoryUsedAccurate" | null;
}

/** What one hold did (`voice-typed`). The words themselves are never sent. */
export interface VoiceTyped {
  outcome: "typed" | "tooShort" | "nothingHeard" | "micBlocked" | "noMic" | "micBusy" | "noTarget" | "keysHeld" | "unavailable" | "failed";
}

/** Where automatic updates are (`updater::policy::Phase`). */
export type UpdatePhase =
  | "notConfigured"
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "ready"
  | "applying"
  | "failed";

/** Which step failed (`updater::policy::Failure`). */
export type UpdateFailure = "check" | "download" | "apply";

/** What an update restart would cut off (`updater::policy::Blocker`). */
export type UpdateBlocker = "call" | "transfer" | "voice";

/** Automatic updates for the Update button and Settings › About (`update_status`, the `update-status` event). */
export interface UpdateStatus {
  /** Counts the changes; the store keeps the highest (`lib/ordering`). Optional only for fixtures. */
  rev?: number;
  phase: UpdatePhase;
  /** The version running now. */
  current: string;
  /** The newer release found, while there is one. */
  version: string | null;
  /** The release notes' first paragraph. */
  headline: string | null;
  /** Download progress in percent, when the size is known. */
  progress: number | null;
  failure: UpdateFailure | null;
  /** The portable exe: it can only point at the download. */
  portable: boolean;
  /** "Download updates automatically". */
  auto: boolean;
  /** When the last check finished (ms since the epoch). */
  checkedAt: number | null;
}

/** Restart now's answer when it didn't restart (`updater::ApplyAnswer`). */
export interface UpdateApplyAnswer {
  blocked: UpdateBlocker | null;
  status: UpdateStatus;
}
