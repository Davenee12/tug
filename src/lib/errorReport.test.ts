import { describe, expect, it } from "vitest";
import { errorName, errorText, firstStackFrame } from "./errorReport";

describe("errorName", () => {
  it("uses the error's constructor name", () => {
    expect(errorName(new TypeError("x"))).toBe("TypeError");
    expect(errorName(new Error("x"))).toBe("Error");
  });
  it("stands in for non-errors", () => {
    expect(errorName("boom")).toBe("Error");
    expect(errorName(42)).toBe("number");
    expect(errorName(null)).toBe("null");
    expect(errorName({ a: 1 })).toBe("object");
  });
});

describe("errorText", () => {
  it("reads the message off an Error", () => {
    expect(errorText(new Error("not a function"))).toBe("not a function");
  });
  it("passes strings through and serialises objects", () => {
    expect(errorText("plain")).toBe("plain");
    expect(errorText({ code: 7 })).toBe('{"code":7}');
  });
});

describe("firstStackFrame", () => {
  it("returns the first real frame of a V8 stack", () => {
    const stack = "TypeError: x is not a function\n    at render (App.vue:12:5)\n    at mount (vue.js:1:1)";
    expect(firstStackFrame(stack)).toBe("at render (App.vue:12:5)");
  });
  it("is empty without a stack", () => {
    expect(firstStackFrame(undefined)).toBe("");
    expect(firstStackFrame(null)).toBe("");
    expect(firstStackFrame("")).toBe("");
  });
});
