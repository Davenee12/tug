//! What was typed after `tug`. Pure, so every verb and mistake is tested.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `tug code [--copy]`
    Code {
        copy: bool,
    },
    /// `tug boat <file...>`
    Boat {
        files: Vec<String>,
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

pub const USAGE: &str = "\
tug: your iPhone, from the terminal (tug must be running)

  tug code [--copy]          print the newest verification code (--copy: also put it on the clipboard)
  tug boat <file>...         send files to your phone with Tugboat
  tug text <name> \"<msg>\"    text someone; tug asks you to confirm first
  tug status                 is tug running, and how's the phone
  tug mcp                    the MCP server for AI tools (they start it; see Settings › Developer tools)

AI tools need Settings › Developer tools › Let AI tools use tug switched on, and so does this command.";

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
        "boat" => {
            if rest.is_empty() {
                return Err("Name the files to send: `tug boat photo.png notes.pdf`.".into());
            }
            Ok(Command::Boat {
                files: rest.iter().map(|s| s.to_string()).collect(),
            })
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
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
                files: vec!["a.png".into(), "b c.pdf".into()]
            })
        );
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
