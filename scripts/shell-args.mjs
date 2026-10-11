// One argument for a command line run through a shell (cmd.exe on Windows, sh elsewhere), so it
// reaches the program exactly as typed.
//
// Escaping for cmd.exe is a trap: it and the program's own argument splitting (the Microsoft C
// runtime's rules) each read quotes and backslashes differently, and an npm .cmd shim runs cmd a
// second time. So rather than escape everything, only characters that mean nothing special to
// either shell are let through, and anything else is refused with an error instead of being
// passed on mangled. That covers what these build scripts take (flags, values, paths).
//
// Inside double quotes the one remaining hazard is backslashes right before the closing quote:
// both the C runtime and sh read `\"` as a literal quote, so a trailing run of backslashes is
// doubled (`"C:\dir\\"` arrives as `C:\dir\`). Backslashes anywhere else are taken literally.

/** Characters that never need quoting: flags, simple values and paths without spaces. */
const BARE = /^[A-Za-z0-9_\-.,:/\\=+@]+$/;
/** Characters allowed inside quotes: the above plus spaces and a few harmless symbols. */
const QUOTABLE = /^[A-Za-z0-9_\-.,:/\\=+@ ~#]*$/;

/**
 * Quote `arg` for the shell, or throw if it holds a character that a shell could act on
 * (`"`, `%`, `&`, `|`, `<`, `>`, `^`, `!`, `$`, backticks, brackets, newlines, …).
 */
export function quoteArg(arg) {
  if (BARE.test(arg)) return arg;
  if (!QUOTABLE.test(arg)) {
    throw new Error(`can't pass ${JSON.stringify(arg)} through the shell safely; use only letters, digits, spaces and - _ . , : / \\ = + @ ~ #`);
  }
  const trailing = arg.length - arg.replace(/\\+$/, "").length;
  return `"${arg}${"\\".repeat(trailing)}"`;
}
