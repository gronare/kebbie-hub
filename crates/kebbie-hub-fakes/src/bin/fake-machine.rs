use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::{Parser, Subcommand, ValueEnum};
use kebbie_hub_fakes::machine::{Backlog, Handled, Handling, MachineState, Posted, connect, every_command, join};
use kebbie_hub_fakes::{load, save};
use kebbie_hub_protocol::{Board, Kind, Outcome};

#[derive(Parser)]
#[command(
    name = "fake-machine",
    about = "Stands in for kebbie on a machine: joins a hub, pins its account, posts boards, applies commands"
)]
struct Cli {
    #[arg(
        long,
        default_value = "fake-machine.json",
        help = "File holding the machine's id, token, keys and pinned account"
    )]
    state: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Join a hub with a code from `kebbie-hub admin join-code`")]
    Join {
        hub: String,
        code: String,
        #[arg(long, default_value = "fake-machine")]
        name: String,
    },
    #[command(
        about = "Connect to a hub without a join code: print a link, wait while someone signs in and approves it"
    )]
    Connect {
        hub: String,
        #[arg(long, default_value = "fake-machine")]
        name: String,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    #[command(about = "Seal a board JSON file to the account the machine pinned and post it")]
    PostBoard {
        file: PathBuf,
        #[arg(long, help = "Revision to post instead of the next one")]
        rev: Option<u64>,
    },
    #[command(about = "Replace the machine's backlog with a JSON file of rows and bodies, seal each body as a blob and post the board")]
    PostBacklog { file: PathBuf },
    #[command(about = "Long-poll the machine's queue once")]
    Poll {
        #[arg(long, default_value_t = 25)]
        wait: u64,
    },
    #[command(
        about = "Apply queued commands: authorize each, print what kebbie would do, post the result; nothing real runs"
    )]
    Apply {
        #[arg(long, default_value_t = 25)]
        wait: u64,
        #[arg(long, help = "Poll once and stop")]
        once: bool,
    },
    #[command(about = "Set what this machine takes from the account's commands; boards posted after say so")]
    Takes { takes: Takes },
    #[command(about = "Take commands from any device of the account, or only from devices approved here")]
    Trust { trust: TrustMode },
    #[command(about = "List the devices that sent commands here and where each stands")]
    Devices,
    #[command(about = "Approve a device by name or id and apply what it sent while it waited")]
    Approve { device: String },
    #[command(about = "Deny a device by name or id and refuse what it sent while it waited")]
    Deny { device: String },
    #[command(about = "Answer each held command as the machine would once its hour ran out unapproved")]
    Expire,
}

#[derive(Clone, Copy, ValueEnum)]
enum TrustMode {
    Account,
    Device,
}

#[derive(Clone, Copy, ValueEnum)]
enum Takes {
    All,
    AnswerAndPush,
    Answer,
    Off,
}

impl Takes {
    fn kinds(self) -> Vec<Kind> {
        match self {
            Takes::All => every_command(),
            Takes::AnswerAndPush => vec![Kind::Answer, Kind::Push, Kind::PromptAnswer],
            Takes::Answer => vec![Kind::Answer, Kind::PromptAnswer],
            Takes::Off => Vec::new(),
        }
    }
}

fn describe(handled: &Handled) -> String {
    let what = match &handled.handling {
        Handling::Applied { did, .. } => format!("applied: {did}"),
        Handling::Refused(result) => format!("refused: {}", outcome(result.outcome)),
        Handling::Replayed(result) => {
            format!("replayed the stored result: {}", outcome(result.outcome))
        }
        Handling::Waiting(device) => format!("held until {device} is approved"),
        Handling::Dropped(dropped) => format!("dropped without a reply: {}", dropped.key()),
    };
    let posted = match handled.posted {
        Some(Posted::Stored(_)) | None => "",
        Some(Posted::AlreadyHeld) => " (the hub already holds its result)",
        Some(Posted::CommandGone) => " (the hub no longer holds the command; result dropped)",
    };
    format!(
        "{} {} {} from {}: {what}{posted}",
        handled.seq,
        handled.kind.name(),
        handled.command,
        handled.from
    )
}

fn outcome(outcome: Outcome) -> String {
    match outcome {
        Outcome::Done => "done".to_string(),
        Outcome::Failed => "failed".to_string(),
        Outcome::Refused { reason } => format!("refused {}", reason.key()),
    }
}

async fn decide(path: &std::path::Path, device: &str, approve: bool) -> Result<(), String> {
    let mut state: MachineState = load(path).map_err(|e| e.to_string())?;
    let id = state
        .device_named(device)
        .ok_or_else(|| format!("no device {device:?} has sent a command here"))?;
    let handled = state
        .decide_device(&id, approve, Some(path))
        .await
        .map_err(|e| e.to_string())?;
    println!("{} {id}", if approve { "approved" } else { "denied" });
    for entry in &handled {
        println!("{}", describe(entry));
    }
    Ok(())
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
        Command::Connect { hub, name, timeout } => {
            if cli.state.exists() {
                return Err(format!(
                    "{} exists; this machine has joined already",
                    cli.state.display()
                ));
            }
            let pending = connect(&hub, &name).await.map_err(|e| text(&e))?;
            println!(
                "Open this link, sign in and press Connect:\n\n    {}\n\nCheck that the page shows {}.",
                pending.link(),
                pending.started.user_code
            );
            let timeout = Duration::from_secs(timeout);
            let (mut state, pairing, secret) = pending.wait(timeout).await.map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("connected to {hub} as {} ({})", state.machine, state.name);
            if let Some(pairing) = pairing {
                let pinned = state
                    .finish_connect(&pairing, &secret, timeout)
                    .await
                    .map_err(|e| text(&e))?;
                save(&cli.state, &state).map_err(|e| text(&e))?;
                println!(
                    "pinned the account's keys (signing {}, encryption {}); the next board is sealed to the account",
                    URL_SAFE_NO_PAD.encode(pinned.signing),
                    URL_SAFE_NO_PAD.encode(pinned.encryption)
                );
            }
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
            println!("posted board rev {posted} sealed to the account");
        }
        Command::PostBacklog { file } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            state.backlog = load::<Backlog>(&file).map_err(|e| text(&e))?;
            let posted = state.publish_backlog().await.map_err(|e| text(&e))?;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("posted board rev {posted} with {} backlog items", state.backlog.rows.len());
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
        Command::Apply { wait, once } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            loop {
                let handled = state
                    .apply_once(wait, Some(&cli.state))
                    .await
                    .map_err(|e| text(&e))?;
                for entry in &handled {
                    println!("{}", describe(entry));
                }
                if once {
                    break;
                }
            }
        }
        Command::Trust { trust } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            state.trust_devices = matches!(trust, TrustMode::Device);
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!("trust: {}", if state.trust_devices { "each device approved here" } else { "any device of the account" });
        }
        Command::Devices => {
            let state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            for (id, known) in &state.devices {
                println!("{id} {} {:?}", known.name, known.standing);
            }
        }
        Command::Approve { device } => decide(&cli.state, &device, true).await?,
        Command::Deny { device } => decide(&cli.state, &device, false).await?,
        Command::Expire => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            for entry in state.expire_held(Some(&cli.state)).await.map_err(|e| text(&e))? {
                println!("{}", describe(&entry));
            }
        }
        Command::Takes { takes } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            state.accepts = takes.kinds();
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let kinds: Vec<&str> = state.accepts.iter().map(|kind| kind.name()).collect();
            println!("takes: {}", if kinds.is_empty() { "nothing".to_string() } else { kinds.join(", ") });
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
