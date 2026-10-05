// Mirrors the serde types in src-tauri/src/{state,store,ams,ancs}.rs.
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
  /** Why message access isn't available, when the user can fix it. */
  messagesError: string | null;
  /** Why the phone's contacts aren't available, when the user can fix it. */
  contactsError: string | null;
}

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
  | "volumeDown";

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
  available: MediaCommand[];
}

export type Transport = "le" | "classic";

export interface DiscoveredDevice {
  id: string;
  name: string;
  transport: Transport;
  paired: boolean;
  connected: boolean;
  canPair: boolean;
}

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
  /** Outgoing: pending → accepted (taken by the iPhone; not proof of delivery) or failed. */
  status: "received" | "pending" | "accepted" | "failed";
}

export interface Contact {
  address: string;
  name: string;
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
}
