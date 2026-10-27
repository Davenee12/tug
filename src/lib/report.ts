// "Report a problem": the diagnostics go on the clipboard (redacted in Rust, for the person to read
// through and paste), then GitHub's bug report form opens, filled in with the version. tug sends
// nothing itself. See src-tauri/src/report.rs.
import { api } from "./ipc";

/** What the notice says after the diagnostics are copied and the form is open. */
export const REPORT_OPENED = "Diagnostics copied. Paste them into the Diagnostics box, and read them through first.";

/** What tug says at startup when its last run (of this version) didn't close normally. */
export const UNCLEAN_EXIT = "tug didn't close normally last time. Tell us about it?";

/** Copy the diagnostics, then open the form. `unclean`: it's about tug not closing normally. */
export async function reportProblem(unclean: boolean): Promise<void> {
  await api.copyDiagnostics();
  await api.reportProblem(unclean);
}
