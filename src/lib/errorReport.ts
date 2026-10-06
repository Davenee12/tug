// Forward uncaught frontend errors to the Rust log so a window crash leaves a trace. The shaping of
// an unknown thrown value into a name/message/stack-top is pure (and unit-tested); `install` wires it
// to window.onerror, unhandled rejections and Vue's error handler. We send only the error's name, a
// message and the top stack frame — never anything the user typed — and the backend redacts and
// rate-limits on top of that.

import type { App } from "vue";
import { api } from "./ipc";

/** The error's constructor name (TypeError, RangeError, …), or a sensible stand-in. */
export function errorName(e: unknown): string {
  if (e instanceof Error) return e.name || "Error";
  if (typeof e === "string") return "Error";
  if (e === null) return "null";
  return typeof e;
}

/** A short human message for the error, whatever shape it arrived in. */
export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    try {
      return JSON.stringify(e).slice(0, 300);
    } catch {
      return Object.prototype.toString.call(e);
    }
  }
  return String(e);
}

/** The top "at …" frame of a stack (where it actually threw), trimmed; "" when there's no stack. */
export function firstStackFrame(stack?: string | null): string {
  if (!stack) return "";
  const lines = stack.split("\n").map((l) => l.trim()).filter(Boolean);
  // V8 stacks lead with the message line; the first "at …" line is the throwing frame.
  return lines.find((l) => l.startsWith("at ")) ?? lines[1] ?? "";
}

/**
 * Wire uncaught errors to the backend. No-op in the plain-browser dev preview (there's no Rust
 * backend to receive them). A re-entrancy guard means a failure inside reporting can't loop.
 */
export function install(app: App): void {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) return;

  let reporting = false;
  const report = (kind: string, e: unknown, source: string) => {
    if (reporting) return;
    reporting = true;
    try {
      const where = source || firstStackFrame(e instanceof Error ? e.stack : null);
      void api.logFrontendError(kind, errorName(e), errorText(e), where).catch(() => {});
    } catch {
      // Reporting must never throw out of here.
    } finally {
      reporting = false;
    }
  };

  window.addEventListener("error", (ev) => {
    const where = firstStackFrame(ev.error instanceof Error ? ev.error.stack : null) || `${ev.filename}:${ev.lineno}`;
    report("error", ev.error ?? ev.message, where);
  });
  window.addEventListener("unhandledrejection", (ev) => report("unhandledrejection", ev.reason, ""));

  // Keep Vue's default console logging; just also forward. `info` is the lifecycle/hook where it blew up.
  const prev = app.config.errorHandler;
  app.config.errorHandler = (err, instance, info) => {
    report("vue", err, info);
    if (prev) prev(err, instance, info);
    else console.error(err);
  };
}
