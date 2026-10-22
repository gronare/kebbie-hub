use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use kelpie_hub_fakes::phone::PhoneState;
use kelpie_hub_fakes::{load, save};
use kelpie_hub_protocol::{Answer, Id, Outcome, Payload, PushApproval, RunRoutine, StartTask};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(
    name = "fake-phone",
    about = "Stands in for the web app: pairs with machines, reads their boards, sends signed commands"
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
    Init {
        hub: String,
        #[arg(
            long,
            requires = "token",
            help = "Device id an account registered; without it only a --dev hub lets the phone pair"
        )]
        device: Option<String>,
        #[arg(long, requires = "device", help = "That device's token")]
        token: Option<String>,
    },
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
    #[command(about = "Answer a task's question, then wait for the machine's result")]
    Answer {
        address: String,
        question: String,
        #[arg(long, help = "0-based option; leave out for a free answer")]
        choice: Option<u8>,
        #[arg(long, default_value = "")]
        comment: String,
        #[command(flatten)]
        target: Target,
    },
    #[command(about = "Approve a push of exactly the head this phone saw on the board")]
    Push {
        address: String,
        #[arg(long, help = "Head to approve instead of the one on the board")]
        head: Option<String>,
        #[command(flatten)]
        target: Target,
    },
    #[command(about = "Start a task in one of the machine's checkouts")]
    Start {
        repo: String,
        name: String,
        prompt: String,
        #[command(flatten)]
        target: Target,
    },
    #[command(
        about = "Subscribe this device to Web Push at an endpoint, with fresh subscription keys kept in the state file"
    )]
    PushSubscribe {
        #[arg(long)]
        endpoint: String,
        #[arg(long, help = "Print the subscription JSON sent, keys included")]
        keys: bool,
    },
    #[command(about = "Remove this device's Web Push subscription")]
    PushUnsubscribe,
    #[command(about = "Run one of the machine's routines now")]
    Routine {
        id: String,
        #[command(flatten)]
        target: Target,
    },
}

#[derive(clap::Args)]
struct Target {
    #[arg(
        long,
        help = "Machine to send to; needed only when the phone paired with several"
    )]
    machine: Option<String>,
    #[arg(long, default_value_t = 60, help = "Seconds to wait for the result")]
    timeout: u64,
}

impl Target {
    fn machine(&self, state: &PhoneState) -> Result<Id, String> {
        match &self.machine {
            Some(machine) => Id::parse(machine).map_err(|e| e.to_string()),
            None => state.only_machine().map_err(|e| e.to_string()),
        }
    }
}

async fn send(
    state: &PhoneState,
    target: &Target,
    machine: &Id,
    payload: Payload,
) -> Result<(), String> {
    let text = |error: &dyn std::fmt::Display| error.to_string();
    let envelope = state
        .command(machine, &payload)
        .await
        .map_err(|e| text(&e))?;
    let command = envelope.header.id;
    println!("sent {} {command}", payload.kind().name());
    let result = state
        .await_result(&command, Duration::from_secs(target.timeout))
        .await
        .map_err(|e| text(&e))?
        .ok_or_else(|| format!("no result for {command} within {} s", target.timeout))?;
    let outcome = match result.outcome {
        Outcome::Done => "done".to_string(),
        Outcome::Failed => "failed".to_string(),
        Outcome::Refused { reason } => format!("refused {}", reason.key()),
    };
    println!("{outcome}: {}", result.text);
    Ok(())
}

async fn board_head(state: &PhoneState, machine: &Id, address: &str) -> Result<String, String> {
    let boards = state.boards().await.map_err(|e| e.to_string())?;
    let board = boards
        .into_iter()
        .find(|entry| &entry.machine == machine)
        .ok_or_else(|| format!("{machine} has posted no board"))?
        .board
        .map_err(|problem| problem.to_string())?;
    board
        .items
        .into_iter()
        .find(|item| item.address == address)
        .and_then(|item| item.head)
        .ok_or_else(|| format!("{address} shows no head on the board"))
}

async fn run(cli: Cli) -> Result<(), String> {
    let text = |error: &dyn std::fmt::Display| error.to_string();
    match cli.command {
        Command::Init { hub, device, token } => {
            if cli.state.exists() {
                return Err(format!("{} exists already", cli.state.display()));
            }
            let state = match (device, token) {
                (Some(device), Some(token)) => {
                    let device = Id::parse(&device).map_err(|e| text(&e))?;
                    PhoneState::registered(&hub, device, &token)
                }
                _ => PhoneState::new(&hub),
            };
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("device keys written to {}", cli.state.display());
        }
        Command::Pair { code, timeout } => {
            let mut state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let outcome = state
                .pair(
                    &code,
                    Duration::from_secs(timeout),
                    async |digits: String| {
                        print!("Same six digits on the machine? {digits} [y/N] ");
                        let _ = std::io::stdout().flush();
                        let mut line = String::new();
                        let mut input = BufReader::new(tokio::io::stdin());
                        input.read_line(&mut line).await.is_ok()
                            && line.trim().eq_ignore_ascii_case("y")
                    },
                )
                .await;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let machine = outcome.map_err(|e| text(&e))?;
            println!("paired with {machine}");
        }
        Command::PushSubscribe { endpoint, keys } => {
            let mut state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let secret = state
                .subscribe_push(&endpoint)
                .await
                .map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            if keys {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&secret.subscription())
                        .expect("a subscription serializes")
                );
            }
            println!("subscribed to push at {endpoint}");
        }
        Command::PushUnsubscribe => {
            let mut state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            state.unsubscribe_push().await.map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("unsubscribed from push");
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
        Command::Answer {
            address,
            question,
            choice,
            comment,
            target,
        } => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let machine = target.machine(&state)?;
            let payload = Payload::Answer(Answer {
                address,
                question,
                choice,
                comment,
            });
            send(&state, &target, &machine, payload).await?;
        }
        Command::Push {
            address,
            head,
            target,
        } => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let machine = target.machine(&state)?;
            let head = match head {
                Some(head) => head,
                None => board_head(&state, &machine, &address).await?,
            };
            let payload = Payload::Push(PushApproval {
                address,
                head,
                card_sha: None,
            });
            send(&state, &target, &machine, payload).await?;
        }
        Command::Start {
            repo,
            name,
            prompt,
            target,
        } => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let machine = target.machine(&state)?;
            let payload = Payload::Start(StartTask { repo, name, prompt });
            send(&state, &target, &machine, payload).await?;
        }
        Command::Routine { id, target } => {
            let state: PhoneState = load(&cli.state).map_err(|e| text(&e))?;
            let machine = target.machine(&state)?;
            send(
                &state,
                &target,
                &machine,
                Payload::Routine(RunRoutine { id }),
            )
            .await?;
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
