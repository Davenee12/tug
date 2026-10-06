import { describe, expect, it } from "vitest";
import { bestTrack, idFromUri, matchPlaylists, matchScore, nextRepeat, normalize, playlistDetail, sameSong, seekFraction, trackLength } from "./spotify";
import type { SpotifyPlaylist, SpotifyTrack } from "../types/protocol";

const pl = (name: string): SpotifyPlaylist => ({ uri: `spotify:playlist:${name}`, id: name, name, owner: "Dave", trackCount: 10, imageUrl: null, owned: true });
const lists = [pl("Deep Focus"), pl("Morning Run"), pl("Discover Weekly"), pl("Coding Flow"), pl("Rainy Day Jazz")];

describe("normalize", () => {
  it("lowercases, strips punctuation/emoji and collapses spaces", () => {
    expect(normalize("  Deep   Focus 🎧 ")).toBe("deep focus");
    expect(normalize("R&B / Soul")).toBe("r b soul");
  });
});

describe("matchScore", () => {
  it("ranks exact over prefix over word-prefix over substring", () => {
    expect(matchScore("deep focus", "Deep Focus")).toBe(100);
    expect(matchScore("deep", "Deep Focus")).toBeGreaterThan(matchScore("focus", "Deep Focus"));
    // "focus" is a whole-word prefix, not a name prefix.
    expect(matchScore("focus", "Deep Focus")).toBe(60);
    // substring that isn't a word start.
    expect(matchScore("ocus", "Deep Focus")).toBe(40);
    expect(matchScore("xyz", "Deep Focus")).toBe(0);
    expect(matchScore("", "Deep Focus")).toBe(0);
  });

  it("matches multiple query words out of order", () => {
    expect(matchScore("day rainy", "Rainy Day Jazz")).toBe(30);
  });
});

describe("matchPlaylists", () => {
  it("returns the best matches, best first", () => {
    expect(matchPlaylists("deep", lists).map((p) => p.name)).toEqual(["Deep Focus"]);
    expect(matchPlaylists("run", lists).map((p) => p.name)).toEqual(["Morning Run"]);
    expect(matchPlaylists("coding flow", lists)[0].name).toBe("Coding Flow");
    expect(matchPlaylists("jazz", lists)[0].name).toBe("Rainy Day Jazz");
    expect(matchPlaylists("nope", lists)).toEqual([]);
  });

  it("respects the limit and prefers the stronger match", () => {
    const many = [pl("Focus"), pl("Deep Focus"), pl("Focus Flow")];
    // Exact "Focus" beats the ones that merely start with it.
    expect(matchPlaylists("focus", many, 2)[0].name).toBe("Focus");
    expect(matchPlaylists("focus", many, 2)).toHaveLength(2);
  });
});

describe("nextRepeat", () => {
  it("cycles off → all → one → off", () => {
    expect(nextRepeat("off")).toBe("all");
    expect(nextRepeat("all")).toBe("one");
    expect(nextRepeat("one")).toBe("off");
    expect(nextRepeat(null)).toBe("all");
  });
});

describe("playlistDetail", () => {
  it("shows owner and count, and leaves out a count Spotify didn't give", () => {
    expect(playlistDetail({ owner: "Dave", trackCount: 42 })).toBe("Dave · 42 songs");
    expect(playlistDetail({ owner: "Dave", trackCount: 1 })).toBe("Dave · 1 song");
    expect(playlistDetail({ owner: "Island Ting", trackCount: null })).toBe("Island Ting");
    expect(playlistDetail({ owner: null, trackCount: 0 })).toBe("0 songs");
  });
});

const track = (name: string, artists = "Artist"): SpotifyTrack => ({
  uri: `spotify:track:${name}`,
  name,
  artists,
  artistUri: "spotify:artist:a",
  album: "Album",
  albumUri: "spotify:album:al",
  imageUrl: null,
  durationMs: 200000,
});

describe("idFromUri", () => {
  it("pulls the bare id from a spotify uri", () => {
    expect(idFromUri("spotify:track:abc123")).toBe("abc123");
    expect(idFromUri("spotify:album:xyz")).toBe("xyz");
    expect(idFromUri(null)).toBeNull();
    expect(idFromUri("")).toBeNull();
  });
});

describe("bestTrack", () => {
  const tracks = [track("Teardrops"), track("Teardrop", "Massive Attack"), track("Tears")];
  it("prefers an exact name match over Spotify's ranking", () => {
    expect(bestTrack("teardrop", tracks)?.name).toBe("Teardrop");
  });
  it("falls back to the top result when nothing matches exactly", () => {
    expect(bestTrack("massive", tracks)?.name).toBe("Teardrops");
  });
  it("matches 'name artist' and a name prefix, and handles empties", () => {
    expect(bestTrack("teardrop massive attack", tracks)?.name).toBe("Teardrop");
    expect(bestTrack("tear", tracks)?.name).toBe("Teardrops");
    expect(bestTrack("anything", [])).toBeNull();
    expect(bestTrack("", tracks)?.name).toBe("Teardrops");
  });
});

describe("trackLength", () => {
  it("formats milliseconds as mm:ss", () => {
    expect(trackLength(0)).toBe("0:00");
    expect(trackLength(5000)).toBe("0:05");
    expect(trackLength(200000)).toBe("3:20");
    expect(trackLength(548000)).toBe("9:08");
  });
});

describe("seekFraction", () => {
  it("maps a pointer x to a 0–1 fraction, clamped to the bar", () => {
    const rect = { left: 100, width: 200 };
    expect(seekFraction(100, rect)).toBe(0);
    expect(seekFraction(200, rect)).toBe(0.5);
    expect(seekFraction(300, rect)).toBe(1);
    expect(seekFraction(50, rect)).toBe(0); // before the start, clamped
    expect(seekFraction(400, rect)).toBe(1); // past the end, clamped
    expect(seekFraction(150, { left: 0, width: 0 })).toBe(0); // zero-width guard
  });
});

describe("sameSong", () => {
  it("matches the track name to the phone's title, ignoring case and surrounding spaces", () => {
    expect(sameSong("Teardrop", "Teardrop")).toBe(true);
    expect(sameSong("  teardrop ", "TEARDROP")).toBe(true);
  });

  it("rejects a different song (Spotify still reporting the previous one)", () => {
    expect(sameSong("Angel", "Teardrop")).toBe(false);
    // Not a fuzzy match: a different version is a different song.
    expect(sameSong("Teardrop - Remastered", "Teardrop")).toBe(false);
  });

  it("is unknown when either side is missing or blank", () => {
    expect(sameSong(null, "Teardrop")).toBeNull();
    expect(sameSong(undefined, "Teardrop")).toBeNull();
    expect(sameSong("   ", "Teardrop")).toBeNull();
    expect(sameSong("Teardrop", null)).toBeNull();
    expect(sameSong("Teardrop", "")).toBeNull();
  });
});
