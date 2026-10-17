import { describe, expect, it } from "vitest";
import { foldChatAppSeen } from "./seen";

const SMS = "com.apple.MobileSMS";

describe("foldChatAppSeen", () => {
  it("moves other chat apps' per-person times onto their Feed stack, newest first", () => {
    const seen = {
      [`${SMS}\u0000jane doe`]: 500,
      "com.whatsapp\u0000sam": 300,
      "com.whatsapp\u0000zoe": 700,
      "com.microsoft.skype.teams\u0000jane doe": 200,
      "app\u0000com.whatsapp": 600,
    };
    expect(foldChatAppSeen(seen)).toEqual({
      [`${SMS}\u0000jane doe`]: 500,
      "app\u0000com.whatsapp": 700,
      "app\u0000com.microsoft.skype.teams": 200,
    });
  });

  it("changes nothing (and so writes nothing) once there's nothing left to fold", () => {
    const seen = { [`${SMS}\u0000jane doe`]: 500, "app\u0000com.whatsapp": 700 };
    expect(foldChatAppSeen(seen)).toBeNull();
    expect(foldChatAppSeen({})).toBeNull();
    expect(foldChatAppSeen(foldChatAppSeen({ "com.whatsapp\u0000sam": 1 })!)).toBeNull();
  });
});
