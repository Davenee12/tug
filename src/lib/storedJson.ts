/**
 * A setting saved as JSON, read back safely: `fallback` when it's missing, isn't JSON, or isn't the
 * shape expected. One damaged value used to throw out of the whole settings load, leaving every
 * setting (Do not disturb, quiet hours, muted apps) at its default until the next restart.
 */
export function storedJson<T>(raw: string | undefined, fallback: T, ok: (v: unknown) => v is T): T {
  if (!raw) return fallback;
  try {
    const v: unknown = JSON.parse(raw);
    return ok(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

export const isStringList = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === "string");

export const isNumberList = (v: unknown): v is number[] =>
  Array.isArray(v) && v.every((x) => typeof x === "number" && Number.isFinite(x));

/** A plain `{…}` object (not null, not a list). */
export const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** `{ key: number }`, as the per-conversation "seen" times are saved. */
export const isNumberMap = (v: unknown): v is Record<string, number> =>
  isObject(v) && Object.values(v).every((x) => typeof x === "number" && Number.isFinite(x));
