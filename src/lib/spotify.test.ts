import { describe, expect, it } from "vitest";
import { matchPlaylists, matchScore, nextRepeat, normalize, playlistDetail } from "./spotify";
import type { SpotifyPlaylist } from "../types/protocol";

const pl = (name: string): SpotifyPlaylist => ({ uri: `spotify:playlist:${name}`, name, owner: "Dave", trackCount: 10, imageUrl: null });
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
