//! `tug`: the command (`tug code`, `tug boat`, `tug text`, `tug status`) and `tug mcp`, the MCP
//! server AI tools start. A small console program shipped inside tug's install folder as
//! `bin\tug.exe`; it talks to the running tug over the local bridge (see `tug-bridge`) and holds
//! no data of its own. See docs/DEVELOPERS.md.

mod args;
mod format;
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
                    _ => {
                        eprintln!("There's no file called {f}.");
                        return format::USAGE;
                    }
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
            eprintln!("Confirm it in tug: Send or Don't send (2 minutes)…");
            let _ = std::io::stderr().flush();
            match call_as::<SendResult>(&c, Call::SendText { to, message }).await {
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
