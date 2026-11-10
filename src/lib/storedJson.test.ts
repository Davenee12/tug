import { describe, expect, it } from "vitest";
import { isNumberList, isNumberMap, isObject, isStringList, storedJson } from "./storedJson";

describe("storedJson", () => {
  it("reads back what was saved", () => {
    expect(storedJson('["a","b"]', [], isStringList)).toEqual(["a", "b"]);
    expect(storedJson('{"zoe":17}', {}, isNumberMap)).toEqual({ zoe: 17 });
    expect(storedJson("[1,2]", [], isNumberList)).toEqual([1, 2]);
  });

  it("falls back when nothing was saved", () => {
    expect(storedJson(undefined, ["x"], isStringList)).toEqual(["x"]);
    expect(storedJson("", [], isStringList)).toEqual([]);
  });

  it("falls back on a damaged value instead of throwing", () => {
    // The bug: this threw out of the settings load, so every setting stayed at its default.
    expect(storedJson('{"zoe":', {}, isNumberMap)).toEqual({});
    expect(storedJson("not json", [], isStringList)).toEqual([]);
  });

  it("falls back on the wrong shape", () => {
    // "null" parsed fine and later broke `.includes` on the muted apps.
    expect(storedJson("null", [], isStringList)).toEqual([]);
    expect(storedJson('{"a":1}', [], isStringList)).toEqual([]);
    expect(storedJson('["a",2]', [], isStringList)).toEqual([]);
    expect(storedJson('{"zoe":"soon"}', {}, isNumberMap)).toEqual({});
    expect(storedJson("[1]", {}, isObject)).toEqual({});
    expect(storedJson('[1,"2"]', [], isNumberList)).toEqual([]);
  });
});
