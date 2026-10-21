use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use kelpie_hub_fakes::phone::PhoneState;
use kelpie_hub_fakes::{load, save};

#[derive(Parser)]
#[command(
    name = "fake-phone",
    about = "Stands in for the web app: pairs with machines, reads their boards"
)]
struct Cli {
    #[arg(
        long,
        default_value = "fake-phone.json",
        help = "File holding the device's keys, token and paired machines"
    )]
    state: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Generate the device's keys for a hub")]
    Init { hub: String },
    #[command(about = "Pair with the machine showing this code")]
    Pair {
        code: String,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    #[command(about = "Fetch, verify and decrypt every paired machine's board")]
    Boards,
    #[command(about = "List every pairing attempt, failed ones included")]
    Attempts,
}

async fn run(cli: Cli) -> Result<(), String> {
    let text = |error: &dyn std::fmt::Display| error.to_string();
    match cli.command {
        Command::Init { hub } => {
            if cli.state.exists() {
                return Err(format!("{} exists already", cli.state.display()));
            }
            save(&cli.state, &PhoneState::new(&hub)).map_err(|e| text(&e))?;
            println!("device keys written to {}", cli.state.display());
        }
        Command::Pair { code, timeout } => {
            let mut state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let outcome = state
                .pair(&code, Duration::from_secs(timeout), |digits| {
                    println!("The machine should show: {digits}");
                })
                .await;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let machine = outcome.map_err(|e| text(&e))?;
            println!("paired with {machine}");
        }
        Command::Boards => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let boards = state.boards().await.map_err(|e| text(&e))?;
            if boards.is_empty() {
                println!("no boards");
            }
            for entry in boards {
                match entry.board {
                    Ok(board) => println!(
                        "{} rev {}\n{}",
                        entry.machine,
                        entry.rev,
                        serde_json::to_string_pretty(&board).expect("a board serializes")
                    ),
                    Err(problem) => println!("{} rev {}: {problem}", entry.machine, entry.rev),
                }
            }
        }
        Command::Attempts => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            for attempt in &state.attempts {
                let machine = attempt
                    .machine
                    .as_ref()
                    .map_or("-".to_string(), ToString::to_string);
                println!("{} {machine} {}", attempt.at, attempt.outcome);
            }
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fake-phone: {message}");
            ExitCode::FAILURE
        }
    }
}
