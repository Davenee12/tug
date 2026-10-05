import { describe, expect, it } from "vitest";
import { findCode } from "./codes";

const code = (t: string | null) => findCode(t)?.code ?? null;

describe("findCode", () => {
  it("finds codes the way services actually send them", () => {
    expect(code("Your code is 482913. Don't share it with anyone.")).toBe("482913");
    expect(code("G-482913 is your Google verification code.")).toBe("482913");
    expect(code("Your Uber code: 4829. Never share this code.")).toBe("4829");
    expect(code("123456 is your Instagram code. Don't share it.")).toBe("123456");
    expect(code("Use 482-913 to verify your account")).toBe("482913");
    expect(code("Chase: Your one-time passcode is 48291337. It expires in 10 minutes.")).toBe("48291337");
    expect(code("Your Microsoft security code is 7731")).toBe("7731");
    expect(code("Sign in to Netflix with 591 204")).toBe("591204");
    expect(code("WhatsApp code 384-221. You can also tap on this link to verify your phone")).toBe("384221");
  });

  it("keeps the text as written for highlighting", () => {
    expect(findCode("Use 482-913 to verify")?.shown).toBe("482-913");
  });

  it("ignores numbers that aren't codes", () => {
    expect(code("Call me at 302-669-8133 when you land")).toBeNull();
    expect(code("meet at 7:30?")).toBeNull();
    expect(code("Order #123456 has shipped")).toBeNull();
    expect(code("Your total is $1234.50")).toBeNull();
    expect(code("omw, 10 mins")).toBeNull();
    expect(code("Verify your number: (302) 669-8133")).toBeNull();
    expect(code("Security alert: sign-in from a new device in 2026")).toBeNull();
    expect(code("")).toBeNull();
    expect(code(null)).toBeNull();
  });

  it("won't guess between two different numbers", () => {
    expect(code("Your code is 482913, or use backup code 771204")).toBeNull();
    expect(code("Code 482913. Again: 482913")).toBe("482913");
  });
});
