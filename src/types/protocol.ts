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

/** UI preferences persisted in SQLite under the `ui.` prefix. */
export interface UiSettings {
  toasts: boolean;
  doNotDisturb: boolean;
  mutedApps: string[];
}
