import { describe, expect, it } from "vitest";
import type { SpotifyDevice } from "../types/protocol";
import {
  activeFromDevices,
  activeFromPlayer,
  connectHint,
  currentDevice,
  isConnectLine,
  pickerLabel,
  pickerRows,
  playingOnLine,
  trackLines,
} from "./playback";

const phone: SpotifyDevice = { id: "ph", name: "Jordan's iPhone", kind: "Smartphone", isActive: false };
const speaker: SpotifyDevice = { id: "spk", name: "Kitchen speaker", kind: "Speaker", isActive: false };
const pc: SpotifyDevice = { id: "pc", name: "Spotify on this PC", kind: "Computer", isActive: false };
const BT = "Jordan's iPhone";

/** Everything a person sees about the playback device, from one set of inputs. */
function view(devices: SpotifyDevice[], api: ReturnType<typeof activeFromDevices>, artist: string | null, chosen: SpotifyDevice | null = null) {
  const hint = connectHint(artist);
  const current = currentDevice(api, hint, devices);
  const rows = pickerRows(devices, current, BT);
  return { current, rows, label: pickerLabel(rows, chosen), marked: rows.filter((r) => r.current).map((r) => r.name), card: playingOnLine(current) };
}

describe("connectHint", () => {
  it("reads the device out of a Spotify Connect artist line", () => {
    expect(connectHint("Listening on Kitchen Echo Dot")).toBe("Kitchen Echo Dot");
    expect(connectHint("  listening on  Living Room TV ")).toBe("Living Room TV");
  });
  it("leaves real artists alone", () => {
    expect(connectHint("Massive Attack")).toBeNull();
    expect(connectHint("Listening On")).toBeNull();
    expect(connectHint(null)).toBeNull();
    expect(connectHint("")).toBeNull();
  });
});

describe("trackLines", () => {
  const connect = { title: "Sweet Music • Voice, Trini Baby", artist: "Listening on Kitchen Echo Dot", album: null };

  it("never shows the Listening on hint as the artist", () => {
    const t = trackLines(connect);
    expect(t.artist).not.toMatch(/listening on/i);
    expect(t).toEqual({ title: "Sweet Music", artist: "Voice, Trini Baby", album: null, hint: "Kitchen Echo Dot" });
  });
  it("prefers Spotify's own names when they describe this song", () => {
    const t = trackLines(connect, { trackName: "Sweet Music", trackArtists: "Voice, Trini Baby" });
    expect([t.title, t.artist]).toEqual(["Sweet Music", "Voice, Trini Baby"]);
    // A song with a bullet in its name: Spotify's split wins over guessing at the bullet.
    const odd = { title: "Intro • Reprise • Some Artist", artist: "Listening on Den", album: null };
    expect(trackLines(odd, { trackName: "Intro • Reprise", trackArtists: "Some Artist" }).title).toBe("Intro • Reprise");
  });
  it("ignores a Spotify snapshot of another song (it lags after a skip)", () => {
    const t = trackLines(connect, { trackName: "Previous Song", trackArtists: "Someone Else" });
    expect([t.title, t.artist]).toEqual(["Sweet Music", "Voice, Trini Baby"]);
  });
  it("hides the artist line when the phone gives nothing but the hint", () => {
    expect(trackLines({ title: "Sweet Music", artist: "Listening on Den", album: null })).toEqual({
      title: "Sweet Music",
      artist: null,
      album: null,
      hint: "Den",
    });
  });
  it("shows ordinary tracks exactly as the phone sent them", () => {
    const np = { title: "A • B", artist: "Massive Attack", album: "Mezzanine" };
    expect(trackLines(np)).toEqual({ ...np, hint: null });
    // Another player's bullet title is its own business.
    expect(trackLines({ ...np, player: "Podcasts" })).toEqual({ ...np, hint: null });
  });
  it("recognises Spotify Connect on an iPhone in another language by its shape", () => {
    for (const artist of ["Écoute sur Cuisine", "Wiedergabe auf Küche", "Escuchando en Salón", "正在 客厅 上收听"]) {
      const np = { title: "Sweet Music • Voice, Trini Baby", artist, album: null, player: "Spotify" };
      expect(isConnectLine(np), artist).toBe(true);
      // The song and artist split out; the device stays unknown (only the English hint names it).
      expect(trackLines(np)).toEqual({ title: "Sweet Music", artist: "Voice, Trini Baby", album: null, hint: null });
    }
  });
  it("keeps a real Spotify artist who is also in the title", () => {
    const np = { title: "Intro • Massive Attack", artist: "Massive Attack", album: null, player: "Spotify" };
    expect(isConnectLine(np)).toBe(false);
    expect(trackLines(np).artist).toBe("Massive Attack");
    expect(isConnectLine({ title: "Teardrop", artist: "Massive Attack", player: "Spotify" })).toBe(false);
  });
});

describe("the playback device: header, Current marker and Now Playing agree", () => {
  it("speaker playing (Dave's case): the speaker everywhere, not the iPhone", () => {
    const devices = [{ ...phone }, { ...speaker, isActive: true }, pc];
    const v = view(devices, activeFromDevices(devices), "Listening on Kitchen speaker");
    expect(v.label).toBe("Kitchen speaker");
    expect(v.marked).toEqual(["Kitchen speaker"]);
    expect(v.card).toBe("on Kitchen speaker");
    expect(v.current?.id).toBe("spk");
  });

  it("phone playing: Spotify's name for the phone, one Current, nothing on the card", () => {
    const spotifyPhone = { ...phone, name: "iPhone", isActive: true }; // Spotify's name differs from Bluetooth's
    const devices = [spotifyPhone, speaker];
    const v = view(devices, activeFromDevices(devices), "Massive Attack");
    expect(v.label).toBe("iPhone");
    expect(v.marked).toEqual(["iPhone"]);
    expect(v.rows[0]).toMatchObject({ phone: true, name: "iPhone" });
    expect(v.card).toBeNull();
  });

  it("no active device: nothing marked Current, the label is where the next play goes", () => {
    const devices = [phone, speaker];
    const v = view(devices, activeFromDevices(devices), "Massive Attack");
    expect(v.marked).toEqual([]);
    expect(v.label).toBe("Jordan's iPhone");
    expect(v.card).toBeNull();
    expect(view(devices, null, null, speaker).label).toBe("Kitchen speaker");
  });

  it("Spotify's API wins over a stale phone hint", () => {
    const devices = [{ ...phone, isActive: true }, speaker];
    const v = view(devices, activeFromDevices(devices), "Listening on Kitchen speaker");
    expect(v.marked).toEqual(["Jordan's iPhone"]);
    expect(v.label).toBe("Jordan's iPhone");
    expect(v.card).toBeNull();
  });

  it("falls back to the phone's hint when Spotify hasn't said (not connected or not read)", () => {
    const v = view([], null, "Listening on Kitchen Echo Dot");
    expect(v.label).toBe("Kitchen Echo Dot");
    expect(v.marked).toEqual(["Kitchen Echo Dot"]);
    expect(v.card).toBe("on Kitchen Echo Dot");
    // The hint matched to a listed device picks up its id and Spotify's spelling.
    expect(currentDevice(null, "kitchen speaker", [phone, speaker])).toEqual({ id: "spk", name: "Kitchen speaker", kind: "Speaker" });
  });

  it("the player read's device works the same as the devices list", () => {
    const api = activeFromPlayer({ deviceId: "spk", deviceName: "Kitchen speaker", deviceKind: "Speaker" });
    const v = view([phone, speaker], api, null);
    expect([v.label, ...v.marked, v.card]).toEqual(["Kitchen speaker", "Kitchen speaker", "on Kitchen speaker"]);
    expect(activeFromPlayer(null)).toBeNull();
  });

  it("never marks two devices Current, and lists the iPhone even when Spotify doesn't", () => {
    const v = view([speaker], null, null);
    expect(v.rows.map((r) => r.name)).toEqual(["iPhone", "Kitchen speaker"]);
    for (const devices of [[phone, speaker], [{ ...phone, isActive: true }, { ...speaker, isActive: true }]]) {
      expect(view(devices, activeFromDevices(devices), "Listening on Kitchen speaker").marked.length).toBeLessThanOrEqual(1);
    }
  });
});
