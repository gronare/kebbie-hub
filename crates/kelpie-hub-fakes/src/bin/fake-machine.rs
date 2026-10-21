use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use kelpie_hub_fakes::machine::{MachineState, join};
use kelpie_hub_fakes::{load, save};
use kelpie_hub_protocol::Board;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(
    name = "fake-machine",
    about = "Stands in for kelpie on a machine: joins a hub, pairs devices, posts boards"
)]
struct Cli {
    #[arg(
        long,
        default_value = "fake-machine.json",
        help = "File holding the machine's id, token, keys and paired devices"
    )]
    state: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Join a hub with a code from `kelpie-hub admin join-code`")]
    Join {
        hub: String,
        code: String,
        #[arg(long, default_value = "fake-machine")]
        name: String,
    },
    #[command(about = "Pair one device: show a code, then confirm the six digits")]
    Pair {
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    #[command(about = "Seal a board JSON file to every paired device and post it")]
    PostBoard {
        file: PathBuf,
        #[arg(long, help = "Revision to post instead of the next one")]
        rev: Option<u64>,
    },
    #[command(about = "Long-poll the machine's queue once")]
    Poll {
        #[arg(long, default_value_t = 25)]
        wait: u64,
    },
}

async fn ask(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let mut input = BufReader::new(tokio::io::stdin());
    input.read_line(&mut line).await.is_ok() && line.trim().eq_ignore_ascii_case("y")
}

async fn run(cli: Cli) -> Result<(), String> {
    let text = |error: &dyn std::fmt::Display| error.to_string();
    match cli.command {
        Command::Join { hub, code, name } => {
            if cli.state.exists() {
                return Err(format!(
                    "{} exists; this machine has joined already",
                    cli.state.display()
                ));
            }
            let state = join(&hub, &code, &name).await.map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("joined {hub} as {} ({})", state.machine, state.name);
        }
        Command::Pair { timeout } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let outcome = state
                .pair(
                    Duration::from_secs(timeout),
                    |code| println!("Enter this code on the phone: {code}"),
                    async |digits: String| {
                        ask(&format!("Same six digits on the phone? {digits} [y/N] ")).await
                    },
                )
                .await;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let device = outcome.map_err(|e| text(&e))?;
            println!("paired {device}; it receives the next board");
        }
        Command::PostBoard { file, rev } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let board: Board = load(&file).map_err(|e| text(&e))?;
            let posted = match rev {
                Some(rev) => state.post_board_at(board, rev).await,
                None => state.post_board(board).await,
            }
            .map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!(
                "posted board rev {posted} sealed to {} device(s)",
                state.devices.len()
            );
        }
        Command::Poll { wait } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let inbox = state.poll_once(wait).await.map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let board = inbox
                .board_rev
                .map_or("none".to_string(), |rev| rev.to_string());
            println!(
                "{} message(s), seq {}, hub holds board rev {board}",
                inbox.messages.len(),
                inbox.seq
            );
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fake-machine: {message}");
            ExitCode::FAILURE
        }
    }
}
