//! Which app started `tug mcp`. An AI tool starts the MCP server as its own child process, so the
//! parent of the tug command at the other end of the pipe is usually the app itself (`Cursor.exe`,
//! `Code.exe`). tug works this out from Windows' process list (`win.rs`), never from anything the
//! caller says, and only uses it when the tool didn't give a usable name of its own. Pure: the
//! process list and start times are passed in.

/// One process, as Windows' process list has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    /// The exe's file name (`Cursor.exe`).
    pub exe: String,
}

/// Apps that start MCP servers themselves, by exe, and the names people know them by. Only apps:
/// a runtime or shell (`node.exe`, `python.exe`, `cmd.exe`) says nothing about which tool it runs.
const KNOWN_LAUNCHERS: &[(&str, &str)] = &[
    ("cursor.exe", "Cursor"),
    ("code.exe", "VS Code"),
    ("code - insiders.exe", "VS Code"),
    ("windsurf.exe", "Windsurf"),
    ("claude.exe", "Claude"),
    ("codex.exe", "Codex"),
    ("zed.exe", "Zed"),
    ("goose.exe", "Goose"),
    ("lm studio.exe", "LM Studio"),
    ("devenv.exe", "Visual Studio"),
];

/// The app an exe belongs to, if it's one tug knows starts AI tools' MCP servers.
pub fn app_name(exe: &str) -> Option<&'static str> {
    let key = exe.trim().to_ascii_lowercase();
    KNOWN_LAUNCHERS.iter().find(|(e, _)| *e == key).map(|(_, name)| *name)
}

/// The tug command: `bin\tug.exe` as installed, `tug-cli.exe` as cargo builds it.
fn is_tug_command(exe: &str) -> bool {
    exe.eq_ignore_ascii_case("tug.exe") || exe.eq_ignore_ascii_case("tug-cli.exe")
}

/// The exe that started `client`, when `client` is the tug command (anything else that opened the
/// pipe wasn't started the way an AI tool starts `tug mcp`) and its parent is still the process
/// that started it. Windows keeps a parent's id after it exits and can hand it to a new process,
/// so a parent that started after its child isn't the one: `started` gives a process's start time
/// (any unit, larger is later), `None` when Windows won't say, and then nothing is claimed.
pub fn launcher(processes: &[Process], client: u32, started: impl Fn(u32) -> Option<u64>) -> Option<String> {
    let me = processes.iter().find(|p| p.pid == client)?;
    if !is_tug_command(&me.exe) || me.parent == 0 || me.parent == me.pid {
        return None;
    }
    let parent = processes.iter().find(|p| p.pid == me.parent)?;
    if started(parent.pid)? > started(me.pid)? {
        return None;
    }
    Some(parent.exe.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32, exe: &str) -> Process {
        Process {
            pid,
            parent,
            exe: exe.into(),
        }
    }

    /// Everything started in pid order.
    fn in_order(pid: u32) -> Option<u64> {
        Some(u64::from(pid))
    }

    #[test]
    fn names_the_app_that_started_tug_mcp() {
        let list = [p(4, 0, "explorer.exe"), p(10, 4, "Cursor.exe"), p(20, 10, "tug.exe")];
        assert_eq!(launcher(&list, 20, in_order).as_deref(), Some("Cursor.exe"));
        // A cargo-built command too.
        let list = [p(10, 4, "Code.exe"), p(20, 10, "tug-cli.exe")];
        assert_eq!(launcher(&list, 20, in_order).as_deref(), Some("Code.exe"));
    }

    #[test]
    fn only_for_the_tug_command() {
        // Something else opened the pipe directly: its parent says nothing about an AI tool.
        let list = [p(10, 4, "Cursor.exe"), p(20, 10, "python.exe")];
        assert_eq!(launcher(&list, 20, in_order), None);
    }

    #[test]
    fn never_a_stranger_that_took_a_dead_parents_id() {
        // The parent exited and Windows gave its id to a newer process.
        let list = [p(30, 4, "Cursor.exe"), p(20, 30, "tug.exe")];
        assert_eq!(launcher(&list, 20, in_order), None);
        // Start times Windows won't give: claim nothing.
        let list = [p(10, 4, "Cursor.exe"), p(20, 10, "tug.exe")];
        assert_eq!(launcher(&list, 20, |_| None), None);
        assert_eq!(launcher(&list, 20, |pid| (pid == 20).then_some(5)), None);
    }

    #[test]
    fn nothing_when_the_parent_is_gone_or_missing() {
        assert_eq!(launcher(&[p(20, 10, "tug.exe")], 20, in_order), None);
        assert_eq!(launcher(&[p(20, 0, "tug.exe")], 20, in_order), None);
        assert_eq!(launcher(&[p(20, 20, "tug.exe")], 20, in_order), None);
        assert_eq!(launcher(&[], 20, in_order), None);
    }

    #[test]
    fn knows_apps_not_runtimes() {
        assert_eq!(app_name("Cursor.exe"), Some("Cursor"));
        assert_eq!(app_name("CODE.EXE"), Some("VS Code"));
        assert_eq!(app_name("Code - Insiders.exe"), Some("VS Code"));
        assert_eq!(app_name("claude.exe"), Some("Claude"));
        assert_eq!(app_name("Windsurf.exe"), Some("Windsurf"));
        // A runtime or a shell runs anything: no name from it.
        for exe in [
            "node.exe",
            "python.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "uv.exe",
            "bash.exe",
        ] {
            assert_eq!(app_name(exe), None, "{exe}");
        }
        assert_eq!(app_name(""), None);
    }
}
