//! `tug`: the command (`tug code`, `tug boat`, `tug text`, `tug status`) and `tug mcp`, the MCP
//! server AI tools start. A small console program shipped inside tug's install folder as
//! `bin\tug.exe`; it talks to the running tug over the local bridge (see `tug-bridge`) and holds
//! no data of its own. See docs/DEVELOPERS.md.

mod args;
mod format;
mod lookup;
mod mcp;
mod send;

use std::io::Write;
use std::process::ExitCode;

use args::Command;
use tug_bridge::client::{time_limit, Client, ClientError};
use tug_bridge::protocol::{
    Call, ClientInfo, ClientKind, CodeResult, OfferResult, Progress, SendResult, StatusResult, StdinFile,
};

fn main() -> ExitCode {
    let argv = args::lossy(std::env::args_os().skip(1));
    let command = match args::parse(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n\n{}", args::USAGE);
            return ExitCode::from(format::USAGE as u8);
        }
    };
    match command {
        Command::Help => {
            println!("{}", args::USAGE);
            return ExitCode::SUCCESS;
        }
        Command::Version => {
            println!("tug {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Couldn't start: {e}");
            return ExitCode::from(format::FAILED as u8);
        }
    };
    let code = runtime.block_on(run(command));
    ExitCode::from(code as u8)
}

fn client(kind: ClientKind) -> Result<Client, ClientError> {
    let name = match kind {
        ClientKind::Cli => "tug command",
        ClientKind::Mcp => "",
    };
    Client::for_current_user(ClientInfo {
        name: name.into(),
        kind,
    })
}

fn fail(e: &ClientError) -> i32 {
    eprintln!("{e}");
    format::exit_code(e)
}

async fn call_as<T: serde::de::DeserializeOwned>(c: &Client, call: Call) -> Result<T, ClientError> {
    c.call_as(&call, time_limit(&call)).await
}

async fn run(command: Command) -> i32 {
    if command == Command::Mcp {
        return serve_mcp().await;
    }
    let c = match client(ClientKind::Cli) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    match command {
        Command::Code { copy } => match call_as::<CodeResult>(&c, Call::GetLatestCode { copy }).await {
            Ok(r) => {
                println!("{}", r.code);
                eprintln!("{}", format::code_detail(&r));
                format::OK
            }
            Err(e) => fail(&e),
        },
        Command::Status => match call_as::<StatusResult>(&c, Call::Status).await {
            Ok(s) => {
                for line in format::status_lines(&s) {
                    println!("{line}");
                }
                format::OK
            }
            Err(e) => fail(&e),
        },
        Command::Boat {
            files,
            stdin_name,
            wait,
        } => boat(&c, &files, stdin_name, wait).await,
        Command::Text { to, message } => text(&c, to, message).await,
        Command::Mcp | Command::Help | Command::Version => format::OK,
    }
}

/// `tug text`: ask tug to send it. Typed in a terminal, it says so, and with "Send `tug text`
/// without asking" on tug sends it straight away; otherwise tug shows its card. tug says which
/// (`Progress`), and this passes it on on stderr; the outcome goes to stdout.
async fn text(c: &Client, to: String, message: String) -> i32 {
    use std::io::IsTerminal;
    let call = Call::SendText {
        to,
        message,
        progress: true,
        typed: std::io::stdin().is_terminal(),
    };
    let mut say = |p: Progress| {
        if let Some(line) = format::text_progress(&p) {
            eprintln!("{line}");
            let _ = std::io::stderr().flush();
        }
    };
    let answer = c.call_with_progress(c.info(), &call, time_limit(&call), &mut say).await;
    match answer.and_then(|v| {
        serde_json::from_value::<SendResult>(v).map_err(|e| ClientError::Other(format!("unexpected answer: {e}")))
    }) {
        Ok(r) => {
            let (line, ok) = format::send_outcome(&r);
            println!("{line}");
            if ok {
                format::OK
            } else {
                format::FAILED
            }
        }
        Err(e) => fail(&e),
    }
}

/// `tug boat`: check the files here (a typo gets a sentence and exit 2 before tug is asked), ask
/// tug, say how it's going on stderr, and the outcome on stdout. 0 once the phone has downloaded
/// everything (or, with `--no-wait`, once it's on offer), 1 if it didn't happen, 2 for bad input.
async fn boat(c: &Client, files: &[String], stdin_name: Option<String>, wait: args::Wait) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => {
            eprintln!("Couldn't tell which folder this is. Give full paths.");
            return format::USAGE;
        }
    };
    let mut paths = Vec::new();
    for f in files {
        match send::resolve(f, &cwd) {
            Ok(path) => paths.push(path),
            // A bare name that isn't here: the file it most likely means, wherever it is.
            Err(tug_bridge::outgoing::Refusal::Missing) => match resolve_loosely(f) {
                Ok(found) => match send::resolve(&found, &cwd) {
                    Ok(path) => paths.push(path),
                    Err(r) => {
                        eprintln!("{}", r.sentence(&found));
                        return format::USAGE;
                    }
                },
                Err(code) => return code,
            },
            Err(r) => {
                eprintln!("{}", r.sentence(f));
                return format::USAGE;
            }
        }
    }
    // What's piped in goes to a file of this command's own, which tug reads once you click Send.
    let mut held_stdin = None;
    let stdin = match stdin_name {
        Some(name) => {
            let bytes = match send::read_stdin() {
                Ok(b) => b,
                Err(p) => {
                    eprintln!("{}", p.sentence());
                    return format::USAGE;
                }
            };
            let file = match send::TempFile::save(&bytes) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Couldn't keep what came in on stdin: {e}");
                    return format::FAILED;
                }
            };
            let s = StdinFile {
                path: file.path(),
                name,
            };
            held_stdin = Some(file);
            Some(s)
        }
        None => None,
    };
    let call = Call::OfferFiles {
        paths,
        stdin,
        wait_secs: wait.wire(),
        progress: true,
    };
    let mut last: Option<String> = None;
    let mut say = |p: Progress| {
        let line = format::boat_progress(&p);
        if last.as_deref() != Some(line.as_str()) {
            eprintln!("{line}");
            let _ = std::io::stderr().flush();
            last = Some(line);
        }
    };
    let answer = c.call_with_progress(c.info(), &call, time_limit(&call), &mut say).await;
    // tug has read its own copy by now (or never will): the command's copy goes.
    drop(held_stdin);
    match answer.and_then(|v| {
        serde_json::from_value::<OfferResult>(v).map_err(|e| ClientError::Other(format!("unexpected answer: {e}")))
    }) {
        Ok(r) => {
            let (line, ok) = format::boat_outcome(&r);
            println!("{line}");
            if ok {
                format::OK
            } else {
                format::FAILED
            }
        }
        Err(e) => fail(&e),
    }
}

/// `tug boat <name>` for a name that isn't a file as given: the one file it most likely means,
/// in this folder, then anywhere Windows Search has indexed, then the usual folders (same name in
/// any case, any extension if none was typed). Says which file it picked; lists the choices
/// (newest first) when there are several. Nothing leaves the PC.
fn resolve_loosely(name: &str) -> Result<String, i32> {
    let cwd = std::env::current_dir().ok();
    match lookup::find(name, cwd.as_deref()) {
        lookup::Found::One(path) => {
            let path = path.to_string_lossy().into_owned();
            eprintln!("Using {path}");
            Ok(path)
        }
        lookup::Found::Many(paths) => {
            eprintln!("Several files match; give the full path or a more exact name.");
            for p in paths.iter().take(lookup::LIST_AT_MOST) {
                eprintln!("  {}", p.display());
            }
            if paths.len() > lookup::LIST_AT_MOST {
                eprintln!("  …and {} more", paths.len() - lookup::LIST_AT_MOST);
            }
            Err(format::USAGE)
        }
        lookup::Found::None => {
            eprintln!(
                "There's no file called {name} on this PC that tug could find. Give the full path, e.g. tug boat \"C:\\path\\to\\file.jpg\""
            );
            Err(format::USAGE)
        }
    }
}

/// `tug mcp`: MCP over stdio until the AI tool closes it. stdout carries only MCP messages;
/// anything for people goes to stderr.
async fn serve_mcp() -> i32 {
    use rmcp::ServiceExt;
    let c = match client(ClientKind::Mcp) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let server = match mcp::TugMcp::new(c).serve(rmcp::transport::stdio()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tug mcp: couldn't start: {e}");
            return format::FAILED;
        }
    };
    match server.waiting().await {
        Ok(_) => format::OK,
        Err(e) => {
            eprintln!("tug mcp: stopped: {e}");
            format::FAILED
        }
    }
}
