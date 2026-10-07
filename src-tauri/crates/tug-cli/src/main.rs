//! `tug`: the command (`tug code`, `tug boat`, `tug text`, `tug status`) and `tug mcp`, the MCP
//! server AI tools start. A small console program shipped inside tug's install folder as
//! `bin\tug.exe`; it talks to the running tug over the local bridge (see `tug-bridge`) and holds
//! no data of its own. See docs/DEVELOPERS.md.

mod args;
mod format;
mod lookup;
mod mcp;

use std::io::Write;
use std::process::ExitCode;

use args::Command;
use tug_bridge::client::{time_limit, Client, ClientError};
use tug_bridge::protocol::{Call, ClientInfo, ClientKind, CodeResult, OfferResult, SendResult, StatusResult};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
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
        Command::Boat { files } => {
            let mut paths = Vec::new();
            for f in &files {
                let p = std::path::Path::new(f);
                match std::path::absolute(p) {
                    Ok(abs) if abs.is_file() => paths.push(abs.to_string_lossy().into_owned()),
                    Ok(abs) if abs.is_dir() => {
                        eprintln!("{f} is a folder. Name the files inside it instead.");
                        return format::USAGE;
                    }
                    _ => match resolve_loosely(f) {
                        Ok(path) => paths.push(path),
                        Err(code) => return code,
                    },
                }
            }
            match call_as::<OfferResult>(&c, Call::OfferFiles { paths }).await {
                Ok(r) => {
                    for line in format::offer_lines(&r) {
                        println!("{line}");
                    }
                    if r.offered > 0 {
                        format::OK
                    } else {
                        format::FAILED
                    }
                }
                Err(e) => fail(&e),
            }
        }
        Command::Text { to, message } => {
            let call = call_as::<SendResult>(&c, Call::SendText { to, message });
            tokio::pin!(call);
            // Only once tug has accepted the request and is showing the card (an error, like the
            // switch being off, comes back at once).
            let answer = tokio::select! {
                r = &mut call => r,
                _ = tokio::time::sleep(std::time::Duration::from_millis(800)) => {
                    eprintln!("Check tug: Send or Don't send (it won't send by itself; 2 minutes)…");
                    let _ = std::io::stderr().flush();
                    call.await
                }
            };
            match answer {
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
        Command::Mcp | Command::Help | Command::Version => format::OK,
    }
}

/// `tug boat <name>` for a name that isn't a file as given: the one file it most likely means, in
/// this folder or Pictures\Tugboat (same name in any case, any extension if none was typed).
/// Says which file it picked; lists the choices when there are several.
fn resolve_loosely(name: &str) -> Result<String, i32> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    if let Some(boat) = lookup::tugboat_dir() {
        dirs.push(boat);
    }
    match lookup::find(name, &dirs) {
        lookup::Found::One(path) => {
            let path = path.to_string_lossy().into_owned();
            eprintln!("Using {path}");
            Ok(path)
        }
        lookup::Found::Many(paths) => {
            eprintln!("More than one file could be {name}:");
            for p in paths.iter().take(lookup::LIST_AT_MOST) {
                eprintln!("  {}", p.display());
            }
            if paths.len() > lookup::LIST_AT_MOST {
                eprintln!("  …and {} more", paths.len() - lookup::LIST_AT_MOST);
            }
            eprintln!("Give the full name or path of the one you mean.");
            Err(format::USAGE)
        }
        lookup::Found::None => {
            eprintln!(
                "There's no file called {name} here or in Pictures\\Tugboat. Give the full path, e.g. tug boat \"C:\\path\\to\\file.jpg\""
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
