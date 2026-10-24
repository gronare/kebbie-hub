use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use kelpie_hub_fakes::machine::{Handled, Handling, MachineState, Posted, connect, join};
use kelpie_hub_fakes::{load, save};
use kelpie_hub_protocol::{Board, Event, Id, Notice, Outcome, Role, Urgency};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(
    name = "fake-machine",
    about = "Stands in for kelpie on a machine: joins a hub, pairs devices, posts boards, applies commands"
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
    #[command(
        about = "Add a phone: print a link to open on it, then confirm the six digits both show"
    )]
    AddDevice {
        #[arg(long, default_value_t = 600)]
        timeout: u64,
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
    #[command(
        about = "Apply queued commands: authorize each, print what kelpie would do, post the result; nothing real runs"
    )]
    Apply {
        #[arg(long, default_value_t = 25)]
        wait: u64,
        #[arg(long, help = "Poll once and stop")]
        once: bool,
    },
    #[command(
        about = "Seal a notice to every paired device and post it; the hub pushes it to the subscribed ones"
    )]
    Notice {
        body: String,
        #[arg(long, default_value = "kelpie")]
        title: String,
        #[arg(long, value_enum, default_value_t = EventName::NeedsYou)]
        event: EventName,
        #[arg(long, value_enum, default_value_t = UrgencyName::Normal)]
        urgency: UrgencyName,
        #[arg(
            long,
            value_name = "TASK@REPO",
            help = "The task the notice is about; its next notice replaces this one"
        )]
        collapse: Option<String>,
        #[arg(
            long,
            requires = "collapse",
            help = "Take back the earlier notice for this task instead"
        )]
        retract: bool,
    },
    #[command(about = "Set what a paired device may do")]
    Role { device: String, role: RoleName },
}

#[derive(Clone, Copy, ValueEnum)]
enum UrgencyName {
    High,
    Normal,
}

#[derive(Clone, Copy, ValueEnum)]
enum EventName {
    NeedsYou,
    Ready,
    Question,
    Summary,
    CiFailed,
    CiGreen,
    ApiError,
    RoutineFindings,
    RoutineFailed,
}

impl EventName {
    fn event(self) -> Event {
        match self {
            EventName::NeedsYou => Event::NeedsYou,
            EventName::Ready => Event::Ready,
            EventName::Question => Event::Question,
            EventName::Summary => Event::Summary,
            EventName::CiFailed => Event::CiFailed,
            EventName::CiGreen => Event::CiGreen,
            EventName::ApiError => Event::ApiError,
            EventName::RoutineFindings => Event::RoutineFindings,
            EventName::RoutineFailed => Event::RoutineFailed,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum RoleName {
    Read,
    Control,
}

fn describe(handled: &Handled) -> String {
    let what = match &handled.handling {
        Handling::Applied { did, .. } => format!("applied: {did}"),
        Handling::Refused(result) => format!("refused: {}", outcome(result.outcome)),
        Handling::Replayed(result) => {
            format!("replayed the stored result: {}", outcome(result.outcome))
        }
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
                let device = state
                    .finish_connect(&pairing, &secret, timeout)
                    .await
                    .map_err(|e| text(&e))?;
                save(&cli.state, &state).map_err(|e| text(&e))?;
                println!("paired the browser {device}; it receives the next board");
            }
        }
        Command::AddDevice { timeout } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let outcome = state
                .add_device(
                    Duration::from_secs(timeout),
                    |added| {
                        println!(
                            "Open this on the phone (or scan it as a QR code):\n\n    {}\n",
                            added.url
                        )
                    },
                    async |digits: String| {
                        ask(&format!("Does the phone show {digits}? [y/N] ")).await
                    },
                )
                .await;
            save(&cli.state, &state).map_err(|e| text(&e))?;
            let device = outcome.map_err(|e| text(&e))?;
            println!("added {device}; it receives the next board");
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
        Command::Notice {
            body,
            title,
            event,
            urgency,
            collapse,
            retract,
        } => {
            let state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let event = event.event();
            let tag = collapse
                .as_deref()
                .map(|address| state.collapse_tag(event, address));
            let notice = Notice {
                event,
                address: collapse,
                title,
                body,
                question: None,
                options: Vec::new(),
                recommend: None,
                board_rev: state.last_rev,
                retract: tag.clone().filter(|_| retract),
            };
            let urgency = match urgency {
                UrgencyName::High => Urgency::High,
                UrgencyName::Normal => Urgency::Normal,
            };
            let posted = state
                .notice(&notice, urgency, tag)
                .await
                .map_err(|e| text(&e))?;
            println!(
                "posted notice {} sealed to {} device(s); the hub pushes it to {}",
                posted.notice,
                state.devices.len(),
                posted.pushes
            );
        }
        Command::Role { device, role } => {
            let mut state: MachineState = load(&cli.state).map_err(|e| text(&e))?;
            let device = Id::parse(&device).map_err(|e| text(&e))?;
            let role = match role {
                RoleName::Read => Role::Read,
                RoleName::Control => Role::Control,
            };
            if !state.set_role(&device, role) {
                return Err(format!("{device} is not a paired device"));
            }
            save(&cli.state, &state).map_err(|e| text(&e))?;
            println!(
                "{device} may now {}",
                match role {
                    Role::Read => "only read",
                    Role::Control => "send commands",
                }
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
