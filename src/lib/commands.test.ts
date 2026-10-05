import { describe, expect, it } from "vitest";
import { parseActions, type Person } from "./commands";

const people: Person[] = [
  { name: "zoe 💜", address: "+13025550173" },
  { name: "Priya", address: "+12145550186" },
  { name: "Jo Smith", address: "+15550000001" },
  { name: "Jo Brown", address: "+15550000002" },
  { name: "Mary Ann", address: "+15550000003" },
  { name: "Mary", address: "+15550000004" },
];

describe("parseActions", () => {
  it("sends a text to someone by name, emoji and all", () => {
    const [a] = parseActions("text zoe running late", people);
    expect(a).toMatchObject({ kind: "send", text: "running late", person: { address: "+13025550173" } });
    expect(a.label).toBe("Send “running late” to zoe 💜");
  });

  it("accepts a few verbs and keeps the message as typed", () => {
    expect(parseActions("tell Priya On my way!", people)[0]).toMatchObject({ text: "On my way!" });
    expect(parseActions("msg priya ok", people)[0]).toMatchObject({ person: { name: "Priya" } });
  });

  it("offers every person a name could mean instead of guessing", () => {
    const actions = parseActions("text jo see you at 5", people);
    expect(actions.map((a) => (a.kind === "send" ? a.person.address : ""))).toEqual(["+15550000001", "+15550000002"]);
    // A full name settles it.
    expect(parseActions("text jo brown see you", people)).toHaveLength(1);
  });

  it("prefers the longest name that fits", () => {
    const [a] = parseActions("text mary ann lunch?", people);
    expect(a).toMatchObject({ person: { name: "Mary Ann" }, text: "lunch?" });
    const mary = parseActions("text mary lunch?", people);
    expect(mary.map((x) => (x.kind === "send" ? x.person.name : ""))).toEqual(["Mary Ann", "Mary"]);
  });

  it("opens the conversation when there's no message yet", () => {
    expect(parseActions("text zoe", people)[0]).toMatchObject({ kind: "open-chat", label: "Message zoe 💜" });
  });

  it("takes a phone number directly", () => {
    expect(parseActions("text 302 555 0100 on my way", people)[0]).toMatchObject({
      kind: "send",
      person: { address: "3025550100", name: "(302) 555-0100" },
      text: "on my way",
    });
  });

  it("knows the media and navigation verbs", () => {
    expect(parseActions("pause", people)).toEqual([{ kind: "media", command: "pause", label: "Pause" }]);
    expect(parseActions("Next", people)[0]).toMatchObject({ command: "nextTrack" });
    expect(parseActions("new message", people)[0]).toMatchObject({ kind: "open", target: "new-message" });
  });

  it("stays out of the way of ordinary searches", () => {
    expect(parseActions("dinner", people)).toEqual([]);
    expect(parseActions("text", people)).toEqual([]);
    expect(parseActions("text nobody hello", people)).toEqual([]);
    expect(parseActions("", people)).toEqual([]);
  });
});
