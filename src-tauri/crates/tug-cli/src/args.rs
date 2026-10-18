//! What was typed after `tug`. Pure, so every verb and mistake is tested.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `tug code [--copy]`
    Code {
        copy: bool,
    },
    /// `tug boat <file...>`, `tug boat - --name <name>`, with `--wait <secs>` or `--no-wait`.
    Boat {
        files: Vec<String>,
        /// `-` was given: send stdin, under this name.
        stdin_name: Option<String>,
        wait: Wait,
    },
    /// `tug text <name> "<message>"`
    Text {
        to: String,
        message: String,
    },
    /// `tug status`
    Status,
    /// `tug mcp`: the MCP server AI tools start (stdio).
    Mcp,
    Help,
    Version,
}

/// How long `tug boat` waits for the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Two minutes.
    Default,
    /// `--wait <secs>`.
    Secs(u32),
    /// `--no-wait`: done once the files are on offer (tug still holds them for two minutes).
    No,
}

impl Wait {
    /// As the bridge takes it (`Call::OfferFiles::wait_secs`).
    pub fn wire(self) -> Option<u32> {
        match self {
            Wait::Default => None,
            Wait::Secs(s) => Some(s),
            Wait::No => Some(0),
        }
    }
}

pub const USAGE: &str = "\
tug: your iPhone, from the terminal (tug must be running)

  tug code [--copy]          print the newest verification code (--copy: also put it on the clipboard)
  tug boat <file>...         send files to your iPhone with Tugboat; tug asks you to confirm first
                             --wait <secs> (default 120, at most 600) or --no-wait
  tug boat - --name <name>   send what's piped in, as a file called <name>
  tug text <name> \"<msg>\"    text someone; tug asks you to confirm first
  tug status                 is tug running, and how's the phone
  tug mcp                    the MCP server for AI tools (they start it; see Settings › Developer tools)

AI tools need Settings › Developer tools › Let AI tools use tug switched on, and so does this command.";

/// The command line as text, never panicking: `std::env::args()` panics on an argument that isn't
/// valid Unicode (a lone UTF-16 surrogate another program passed), and the release build aborts on
/// a panic. Such an argument is read lossily (U+FFFD for what can't be decoded), so it's simply an
/// unknown argument and gets the usage.
pub fn lossy(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<String> {
    args.into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// Parse the arguments after the program name. `Err` is a sentence to print with the usage.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter().map(String::as_str);
    let Some(verb) = it.next() else {
        return Ok(Command::Help);
    };
    let rest: Vec<&str> = it.collect();
    let no_more = |cmd: Command| {
        if rest.is_empty() {
            Ok(cmd)
        } else {
            Err(format!("`tug {verb}` doesn't take {:?}.", rest[0]))
        }
    };
    match verb {
        "help" | "--help" | "-h" | "/?" => Ok(Command::Help),
        "version" | "--version" | "-V" => no_more(Command::Version),
        "status" => no_more(Command::Status),
        "mcp" => no_more(Command::Mcp),
        "code" => match rest.as_slice() {
            [] => Ok(Command::Code { copy: false }),
            ["--copy" | "-c"] => Ok(Command::Code { copy: true }),
            [other, ..] => Err(format!(
                "`tug code` doesn't take {other:?}. Did you mean `tug code --copy`?"
            )),
        },
        "boat" => parse_boat(&rest),
        "text" => match rest.as_slice() {
            [] | [_] => Err("Say who and what: `tug text Sam \"On my way\"`.".into()),
            [to, message @ ..] => {
                let message = message.join(" ");
                if to.trim().is_empty() || message.trim().is_empty() {
                    return Err("Say who and what: `tug text Sam \"On my way\"`.".into());
                }
                Ok(Command::Text {
                    to: to.to_string(),
                    message,
                })
            }
        },
        other => Err(format!("tug doesn't know {other:?}.")),
    }
}

const WAIT_HELP: &str = "`--wait` takes seconds from 1 to 600, e.g. `--wait 300`.";

/// `tug boat`'s files and options, in any order. `--` ends the options (for a file whose name
/// starts with `-`).
fn parse_boat(rest: &[&str]) -> Result<Command, String> {
    use tug_bridge::protocol::{MAX_OFFER_FILES, MAX_WAIT_SECS};
    let mut files = Vec::new();
    let mut stdin = false;
    let mut name: Option<String> = None;
    let mut wait: Option<u32> = None;
    let mut no_wait = false;
    let mut options = true;
    let mut it = rest.iter().copied();
    let seconds = |v: Option<&str>| -> Result<u32, String> {
        v.and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|s| (1..=MAX_WAIT_SECS).contains(s))
            .ok_or_else(|| WAIT_HELP.to_string())
    };
    while let Some(a) = it.next() {
        if !options {
            files.push(a.to_string());
            continue;
        }
        match a {
            "--" => options = false,
            "-" if stdin => return Err("Give `-` (stdin) only once.".into()),
            "-" => stdin = true,
            "--name" => name = Some(it.next().unwrap_or_default().to_string()),
            "--wait" => wait = Some(seconds(it.next())?),
            "--no-wait" => no_wait = true,
            _ if a.starts_with("--name=") => name = Some(a["--name=".len()..].to_string()),
            _ if a.starts_with("--wait=") => wait = Some(seconds(Some(&a["--wait=".len()..]))?),
            _ if a.starts_with('-') => {
                return Err(format!(
                    "`tug boat` doesn't know {a:?}. For a file whose name starts with -, put -- before it."
                ))
            }
            _ => files.push(a.to_string()),
        }
    }
    let name = name.map(|n| n.trim().to_string());
    match (stdin, &name) {
        (true, None) => return Err("Name what you pipe in: `tug boat - --name notes.txt`.".into()),
        (true, Some(n)) if n.is_empty() => return Err("Name what you pipe in: `tug boat - --name notes.txt`.".into()),
        (false, Some(_)) => return Err("`--name` names what you pipe in: `tug boat - --name notes.txt`.".into()),
        _ => {}
    }
    if files.is_empty() && !stdin {
        return Err("Name the files to send: `tug boat photo.png notes.pdf`.".into());
    }
    if files.len() + usize::from(stdin) > MAX_OFFER_FILES {
        return Err(format!("Send at most {MAX_OFFER_FILES} files at a time."));
    }
    let wait = match (wait, no_wait) {
        (Some(_), true) => return Err("Use `--wait` or `--no-wait`, not both.".into()),
        (Some(s), false) => Wait::Secs(s),
        (None, true) => Wait::No,
        (None, false) => Wait::Default,
    };
    Ok(Command::Boat {
        files,
        stdin_name: name.filter(|_| stdin),
        wait,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_non_unicode_argument_is_read_lossily_not_a_panic() {
        use std::ffi::OsString;
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x0041, 0xD800, 0x0042])
        };
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'A', 0xFF, b'B'])
        };
        assert!(odd.to_str().is_none(), "the test argument really isn't Unicode");
        let argv = lossy([OsString::from("status"), odd]);
        assert_eq!(argv, vec!["status".to_string(), "A\u{FFFD}B".to_string()]);
        assert!(parse(&argv).is_err(), "an unknown argument, not a crash");
        assert_eq!(parse(&lossy([OsString::from("status")])), Ok(Command::Status));
    }

    #[test]
    fn verbs() {
        assert_eq!(p(&[]), Ok(Command::Help));
        assert_eq!(p(&["--help"]), Ok(Command::Help));
        assert_eq!(p(&["code"]), Ok(Command::Code { copy: false }));
        assert_eq!(p(&["code", "--copy"]), Ok(Command::Code { copy: true }));
        assert_eq!(p(&["status"]), Ok(Command::Status));
        assert_eq!(p(&["mcp"]), Ok(Command::Mcp));
        assert_eq!(p(&["--version"]), Ok(Command::Version));
        assert_eq!(
            p(&["boat", "a.png", "b c.pdf"]),
            Ok(Command::Boat {
                files: vec!["a.png".into(), "b c.pdf".into()],
                stdin_name: None,
                wait: Wait::Default,
            })
        );
    }

    fn boat(args: &[&str]) -> Result<(Vec<String>, Option<String>, Wait), String> {
        let mut all = vec!["boat"];
        all.extend_from_slice(args);
        match p(&all)? {
            Command::Boat {
                files,
                stdin_name,
                wait,
            } => Ok((files, stdin_name, wait)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn boat_takes_waits_in_any_order() {
        assert_eq!(
            boat(&["report.pdf", "--wait", "300"]),
            Ok((vec!["report.pdf".into()], None, Wait::Secs(300)))
        );
        assert_eq!(
            boat(&["--wait=45", "a.png", "b.png"]),
            Ok((vec!["a.png".into(), "b.png".into()], None, Wait::Secs(45)))
        );
        assert_eq!(boat(&["--no-wait", "a.png"]).unwrap().2, Wait::No);
        assert_eq!(Wait::Default.wire(), None);
        assert_eq!(Wait::Secs(30).wire(), Some(30));
        assert_eq!(Wait::No.wire(), Some(0));
        for bad in [
            &["a.png", "--wait"][..],
            &["a.png", "--wait", "0"],
            &["a.png", "--wait", "601"],
            &["a.png", "--wait", "soon"],
            &["a.png", "--wait=-5"],
        ] {
            assert!(boat(bad).unwrap_err().contains("--wait"), "{bad:?}");
        }
        assert!(boat(&["a.png", "--wait", "5", "--no-wait"])
            .unwrap_err()
            .contains("not both"));
    }

    #[test]
    fn boat_sends_stdin_under_a_name() {
        assert_eq!(
            boat(&["-", "--name", "notes.txt"]),
            Ok((vec![], Some("notes.txt".into()), Wait::Default))
        );
        assert_eq!(
            boat(&["--name=log.txt", "-", "a.png"]),
            Ok((vec!["a.png".into()], Some("log.txt".into()), Wait::Default))
        );
        assert!(boat(&["-"]).unwrap_err().contains("--name"));
        assert!(boat(&["-", "--name", "  "]).unwrap_err().contains("--name"));
        assert!(boat(&["a.png", "--name", "x.png"]).unwrap_err().contains("pipe"));
        assert!(boat(&["-", "-", "--name", "x"]).is_err());
    }

    #[test]
    fn boat_mistakes_get_a_sentence() {
        assert!(boat(&[]).unwrap_err().contains("Name the files"));
        assert!(boat(&["--no-wait"]).unwrap_err().contains("Name the files"));
        assert!(boat(&["--force", "a.png"]).unwrap_err().contains("--"));
        // A file whose name starts with a dash, after `--`.
        assert_eq!(boat(&["--", "--wait"]).unwrap().0, vec!["--wait".to_string()]);
        let many: Vec<String> = (0..21).map(|i| format!("f{i}.txt")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(boat(&many).unwrap_err().contains("at most 20"));
    }

    #[test]
    fn text_takes_a_name_and_the_rest_as_the_message() {
        assert_eq!(
            p(&["text", "Sam", "On my way"]),
            Ok(Command::Text {
                to: "Sam".into(),
                message: "On my way".into()
            })
        );
        // Unquoted words are joined, as people type them.
        assert_eq!(
            p(&["text", "Sam", "running", "late"]),
            Ok(Command::Text {
                to: "Sam".into(),
                message: "running late".into()
            })
        );
    }

    #[test]
    fn mistakes_get_a_sentence() {
        assert!(p(&["text", "Sam"]).is_err());
        assert!(p(&["text", "Sam", "  "]).is_err());
        assert!(p(&["boat"]).is_err());
        assert!(p(&["code", "--paste"]).unwrap_err().contains("--copy"));
        assert!(p(&["status", "now"]).is_err());
        assert!(p(&["drop", "x"]).unwrap_err().contains("drop"));
    }
}
