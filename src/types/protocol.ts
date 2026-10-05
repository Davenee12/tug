// Mirrors the serde types in src-tauri/src/{state,store,ams,ancs,map/calls,toast}.rs.
// Keep field names and enum strings in sync with the Rust side.

export type RadioState = "unknown" | "on" | "off" | "unavailable";
export type AdvertisingState = "off" | "starting" | "on" | "error";
export type ConnectionState = "noDevice" | "disconnected" | "connecting" | "connected";

export interface PairedDevice {
  id: string;
  name: string;
}

export interface Services {
  notifications: boolean;
  media: boolean;
  battery: boolean;
  /** Bluetooth MAP session (read inbox, send replies) is up. */
  messages: boolean;
}

export interface DeviceStatus {
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
  /** Why message access isn't available, when the user can fix it. */
  messagesError: string | null;
  /** Why the phone's contacts aren't available, when the user can fix it. */
  contactsError: string | null;
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
  | "advanceRepeatMode";

export interface NowPlaying {
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
  /** Phone-local ISO time, when the phone reported one. */
  sentAt: string | null;
  receivedAt: number;
  /**
   * Outgoing: pending → accepted (taken by the iPhone; not proof of delivery) → sent (a MAP
   * SendingSuccess event confirmed it left), or failed. Without live texts a send stops at
   * accepted; the UI shows both accepted and sent as "Sent".
   */
  status: "received" | "pending" | "accepted" | "sent" | "failed";
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

/** UI preferences persisted in SQLite under the `ui.` prefix. */
export interface UiSettings {
  toasts: boolean;
  doNotDisturb: boolean;
  mutedApps: string[];
  /** Closing the window keeps tug running in the tray (read by the backend too). */
  closeToTray: boolean;
  /** Real app icons in the Feed, fetched once per app from Apple's App Store. */
  appIcons: boolean;
  /** A Windows pop-up when the iPhone's battery drops to 20% and 10%. */
  lowBattery: boolean;
  /** Experimental Call buttons. Only a successful hands-free check (Settings › iPhone) turns them on. */
  dialing: boolean;
  /** Texts from unknown senders go to their own list in Messages: no badge, no pop-up (codes still pop up). */
  filterUnknown: boolean;
  /** Numbers/emails moved to conversations by hand ("Move to conversations"), normalised. */
  knownSenders: string[];
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
  /** A conversation: "Mark read". */
  markRead: boolean;
  /** A one-time code: "Copy code". */
  code: string | null;
  /** A missed call still on the phone with its Dial action: "Call back". */
  callBack: boolean;
  /** Clearable on the phone (never a ringing call): "Clear". */
  clear: boolean;
}

/** A pop-up's body or button was pressed and carried out (`toast-pressed`, `toast/mod.rs`). */
export interface ToastPressed {
  kind: "open" | "read" | "replied" | "copied" | "calledBack";
  id: number;
}

// --- Spotify connector (mirrors src-tauri/src/spotify/{mod,model}.rs) ---

/** Spotify connection state for Settings. */
export interface SpotifyStatus {
  connected: boolean;
  /** The connected account's display name, when connected. */
  account: string | null;
  /** The Client ID the owner pasted (null until set). */
  clientId: string | null;
  /** The exact Redirect URI to register in the Spotify dashboard. */
  redirectUri: string;
}

/** One of the user's own or followed playlists. */
export interface SpotifyPlaylist {
  /** `spotify:playlist:…`, played as the `context_uri`. */
  uri: string;
  name: string;
  owner: string | null;
  trackCount: number;
}

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
  deviceName: string | null;
}
