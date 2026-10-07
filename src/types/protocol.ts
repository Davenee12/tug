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
  /** tug is rebuilding the link on its own (Bluetooth stalled, the PC woke): say "Reconnecting…", not "Waiting". */
  reconnecting: boolean;
  /** Why message access isn't available, when the user can fix it. */
  messagesError: string | null;
  /** Why the phone's contacts aren't available, when the user can fix it. */
  contactsError: string | null;
  /** The phone shared contacts on the current connection (Sync Contacts on). */
  contactsShared: boolean;
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
  | "advanceRepeatMode"
  | "skipForward"
  | "skipBackward"
  | "likeTrack"
  | "dislikeTrack";

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
  /** Phone-local ISO time, when the phone reported one. */
  sentAt: string | null;
  receivedAt: number;
  /**
   * Outgoing: pending → accepted (taken by the iPhone; not proof of delivery) → sent (a MAP
   * SendingSuccess event confirmed it left), or failed. Without live texts a send stops at
   * accepted; the UI shows both accepted and sent as "Sent".
   */
  status: "received" | "pending" | "accepted" | "sent" | "failed";
  /**
   * The MAP message type the phone reported (SMS_GSM, SMS_CDMA, MMS, EMAIL, IM), for telling
   * iMessage (IM) from a plain text. Null for history from before tug stored it, and for sends.
   */
  msgType: string | null;
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
  deviceName: string | null;
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
