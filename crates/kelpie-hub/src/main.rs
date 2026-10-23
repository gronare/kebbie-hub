use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::{Parser, Subcommand, ValueEnum};
use kelpie_hub::store::{KeyStatus, Store, StoredReport};
use kelpie_hub::{
    Config, DEFAULT_REPORT_DIFFICULTY, DEV_ORIGIN, DEV_RP_ID, DEV_VAPID_SUBJECT, Hub, TrustedProxy,
    database_path, mint_admin_token, mint_invite, mint_join_code, serve,
};
use kelpie_hub_protocol::Id;

const DEV_LISTEN: &str = "127.0.0.1:8470";

#[derive(Parser)]
#[command(
    name = "kelpie-hub",
    version,
    about = "The kelpie hub: relays sealed boards between machines and devices"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Use localhost and a data directory under the system temp directory"
    )]
    dev: bool,
    #[arg(long, global = true, help = "Directory holding the hub's sqlite file")]
    data: Option<PathBuf>,
    #[arg(long, help = "Address to listen on")]
    listen: Option<SocketAddr>,
    #[arg(
        long,
        value_enum,
        help = "Take the client address from this proxy's header instead of the TCP peer"
    )]
    trusted_proxy: Option<Proxy>,
    #[arg(
        long,
        help = "Passkey relying party id: the hub's domain, like hub.example.com"
    )]
    rp_id: Option<String>,
    #[arg(
        long,
        help = "The origin people open the hub at, like https://hub.example.com"
    )]
    origin: Option<String>,
    #[arg(
        long,
        help = "Contact the push services see in every VAPID token: mailto:you@example.com or the hub's https: URL"
    )]
    vapid_subject: Option<String>,
    #[arg(
        long,
        help = "File holding the hub's VAPID private key, created on first start [default: <data>/vapid.key]"
    )]
    vapid_key: Option<PathBuf>,
    #[arg(
        long = "push-allow-host",
        value_name = "HOST",
        help = "Accept push endpoints on this host besides the known push services; *.example.com matches subdomains, and a loopback host may use http"
    )]
    push_allow_hosts: Vec<String>,
    #[arg(
        long,
        value_name = "BITS",
        default_value_t = DEFAULT_REPORT_DIFFICULTY,
        help = "Leading zero bits a report's proof of work must reach; each bit doubles a sender's work"
    )]
    report_difficulty: u8,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Proxy {
    Cloudflare,
}

#[derive(Subcommand)]
enum Command {
    #[command(subcommand, about = "Operator tasks against the hub's data directory")]
    Admin(Admin),
}

#[derive(Subcommand)]
enum Admin {
    #[command(about = "Mint a one-time code a machine joins with")]
    JoinCode,
    #[command(about = "Mint a one-time code a person signs up with")]
    Invite,
    #[command(about = "List the machines that joined")]
    Machines,
    #[command(about = "Remove a machine, its board, queue and admissions")]
    RemoveMachine { machine: String },
    #[command(
        about = "Mint a named token that reads and acknowledges allowed reports over the API, or list or revoke them"
    )]
    Token {
        #[arg(long, help = "The token's name, such as the machine it goes to")]
        name: Option<String>,
        #[command(subcommand)]
        action: Option<TokenAction>,
    },
    #[command(about = "List error reports from kelpie installs, or decide on a sender's key")]
    Reports {
        #[arg(long, help = "List the reports from keys not yet allowed")]
        quarantine: bool,
        #[arg(long, help = "Print JSON")]
        json: bool,
        #[command(subcommand)]
        action: Option<ReportAction>,
    },
}

#[derive(Subcommand)]
enum TokenAction {
    #[command(about = "List the tokens by name, with when each was made and last used")]
    List,
    #[command(about = "Revoke a token by its name; it stops working at once")]
    Revoke { name: String },
}

fn token(
    data: &std::path::Path,
    name: Option<&str>,
    action: Option<&TokenAction>,
) -> Result<(), String> {
    let store = open_store(data)?;
    let failed = |error: rusqlite::Error| error.to_string();
    match action {
        Some(TokenAction::List) => {
            for (name, created, used) in store.admin_tokens().map_err(failed)? {
                let used = used.map_or("never used".to_string(), |at| format!("used {at}"));
                println!("{name}  made {created}  {used}");
            }
        }
        Some(TokenAction::Revoke { name }) => {
            if !store.revoke_admin_token(name).map_err(failed)? {
                return Err(format!(
                    "no token named {name}; kelpie-hub admin token list names them"
                ));
            }
            println!("revoked {name}");
        }
        None => {
            let name = name.and_then(kelpie_hub::clean_name).ok_or(
                "kelpie-hub admin token needs --name <name>, such as the machine it goes to",
            )?;
            let token = mint_admin_token(&store, name)
                .map_err(failed)?
                .ok_or_else(|| {
                    format!("a token named {name} exists; revoke it first or pick another name")
                })?;
            println!("{token}");
            eprintln!(
                "{name}: shown once; it reads and acknowledges allowed reports at /v1/admin/reports"
            );
        }
    }
    Ok(())
}

#[derive(Subcommand)]
enum ReportAction {
    #[command(about = "Let a sender's reports through, the waiting ones included")]
    Allow { key: String },
    #[command(about = "Drop a sender's reports, the waiting ones and every later one")]
    Block { key: String },
    #[command(about = "Mark a report handled, so the API no longer hands it out")]
    Ack { id: u64 },
}

fn report_key(text: &str) -> Result<[u8; 32], String> {
    URL_SAFE_NO_PAD
        .decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| format!("{text} is not a report key; kelpie-hub admin reports lists them"))
}

fn report_line(stored: &StoredReport) -> String {
    let acked = stored
        .acked_at
        .map_or(String::new(), |at| format!(" acked {at}"));
    format!(
        "{}  {}  {}  {}  {}{acked}\n    {} · {}",
        stored.id,
        stored.received_at,
        stored.status.name(),
        URL_SAFE_NO_PAD.encode(stored.key),
        stored.report.title,
        stored.report.version,
        stored.report.os
    )
}

fn report_json(stored: &StoredReport) -> serde_json::Value {
    serde_json::json!({
        "id": stored.id,
        "key": URL_SAFE_NO_PAD.encode(stored.key),
        "status": stored.status.name(),
        "received_at": stored.received_at,
        "acked_at": stored.acked_at,
        "title": stored.report.title,
        "text": stored.report.text,
        "version": stored.report.version,
        "os": stored.report.os,
    })
}

fn reports(
    data: &std::path::Path,
    quarantine: bool,
    json: bool,
    action: Option<&ReportAction>,
) -> Result<(), String> {
    let store = open_store(data)?;
    let at = kelpie_hub::now();
    let failed = |error: rusqlite::Error| error.to_string();
    match action {
        Some(ReportAction::Allow { key }) => {
            store
                .set_key_status(&report_key(key)?, KeyStatus::Allowed, at)
                .map_err(failed)?;
            println!("allowed {key}; its reports reach the API");
        }
        Some(ReportAction::Block { key }) => {
            store
                .set_key_status(&report_key(key)?, KeyStatus::Blocked, at)
                .map_err(failed)?;
            println!("blocked {key}; its reports are dropped");
        }
        Some(ReportAction::Ack { id }) => {
            if !store.ack_report(*id, at).map_err(failed)? {
                return Err(format!("no allowed report {id}"));
            }
            println!("acknowledged {id}");
        }
        None => {
            let status = if quarantine {
                KeyStatus::Quarantine
            } else {
                KeyStatus::Allowed
            };
            let listed = store
                .reports(status, 0, !quarantine, u64::MAX >> 1)
                .map_err(failed)?;
            if json {
                let rows: Vec<serde_json::Value> = listed.iter().map(report_json).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rows).map_err(|error| error.to_string())?
                );
            } else {
                for stored in &listed {
                    println!("{}", report_line(stored));
                }
                if !quarantine {
                    let waiting = store.quarantined_count().map_err(failed)?;
                    if waiting > 0 {
                        eprintln!(
                            "{waiting} more wait in quarantine: kelpie-hub admin reports --quarantine"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn data_dir(cli: &Cli) -> Result<PathBuf, String> {
    match (&cli.data, cli.dev) {
        (Some(dir), _) => Ok(dir.clone()),
        (None, true) => Ok(std::env::temp_dir().join("kelpie-hub-dev")),
        (None, false) => Err("pass --data <dir>, or --dev for a local hub".to_string()),
    }
}

fn listen_address(cli: &Cli) -> Result<SocketAddr, String> {
    match (cli.listen, cli.dev) {
        (Some(address), true) if !address.ip().is_loopback() => Err(format!(
            "--dev lets devices in without an account; listen on a loopback address, not {address}"
        )),
        (Some(address), _) => Ok(address),
        (None, true) => Ok(DEV_LISTEN.parse().expect("the dev address parses")),
        (None, false) => Err("pass --listen <address>, or --dev for a local hub".to_string()),
    }
}

fn relying_party_config(cli: &Cli) -> Result<(String, String), String> {
    match (&cli.rp_id, &cli.origin, cli.dev) {
        (Some(rp_id), Some(origin), _) => Ok((rp_id.clone(), origin.clone())),
        (None, None, true) => Ok((DEV_RP_ID.to_string(), DEV_ORIGIN.to_string())),
        _ => Err("pass --rp-id <domain> and --origin <url>, or --dev for a local hub".to_string()),
    }
}

fn vapid_subject(cli: &Cli) -> Result<String, String> {
    match (&cli.vapid_subject, cli.dev) {
        (Some(subject), _) => Ok(subject.clone()),
        (None, true) => Ok(DEV_VAPID_SUBJECT.to_string()),
        (None, false) => Err(
            "pass --vapid-subject mailto:<you@your-domain> (or the hub's https: URL), or --dev for a local hub"
                .to_string(),
        ),
    }
}

fn open_store(data: &std::path::Path) -> Result<Store, String> {
    Store::open(&database_path(data)).map_err(|error| error.to_string())
}

async fn run(cli: Cli) -> Result<(), String> {
    let data = data_dir(&cli)?;
    std::fs::create_dir_all(&data).map_err(|error| format!("{}: {error}", data.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&data, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|error| format!("{}: {error}", data.display()))?;
    let defaults = Config::new(&data);
    match &cli.command {
        Some(Command::Admin(Admin::JoinCode)) => {
            let code = mint_join_code(&open_store(&data)?, defaults.join_code_ttl)
                .map_err(|error| error.to_string())?;
            println!("{code}");
            eprintln!("valid for {} minutes, once", defaults.join_code_ttl / 60);
            Ok(())
        }
        Some(Command::Admin(Admin::Invite)) => {
            let code = mint_invite(&open_store(&data)?, defaults.invite_ttl)
                .map_err(|error| error.to_string())?;
            println!("{code}");
            eprintln!("valid for {} days, once", defaults.invite_ttl / 86_400);
            Ok(())
        }
        Some(Command::Admin(Admin::Machines)) => {
            let machines = open_store(&data)?
                .machines()
                .map_err(|error| error.to_string())?;
            for machine in machines {
                let seen = machine
                    .last_seen
                    .map_or("-".to_string(), |at| at.to_string());
                println!(
                    "{} {} joined {} seen {seen}",
                    machine.id, machine.name, machine.joined_at
                );
            }
            Ok(())
        }
        Some(Command::Admin(Admin::Token { name, action })) => {
            token(&data, name.as_deref(), action.as_ref())
        }
        Some(Command::Admin(Admin::Reports {
            quarantine,
            json,
            action,
        })) => reports(&data, *quarantine, *json, action.as_ref()),
        Some(Command::Admin(Admin::RemoveMachine { machine })) => {
            let machine = Id::parse(machine).map_err(|error| error.to_string())?;
            let store = open_store(&data)?;
            if !store
                .remove_machine(&machine)
                .map_err(|error| error.to_string())?
            {
                return Err(format!("no machine {machine}"));
            }
            store
                .log(
                    kelpie_hub::now(),
                    "machine.removed",
                    Some(&machine),
                    None,
                    None,
                )
                .map_err(|error| error.to_string())?;
            println!("removed {machine}");
            Ok(())
        }
        None => {
            let address = listen_address(&cli)?;
            let (rp_id, origin) = relying_party_config(&cli)?;
            let mut config = defaults;
            config.rp_id = rp_id;
            config.origin = origin;
            config.vapid_subject = vapid_subject(&cli)?;
            if let Some(key) = &cli.vapid_key {
                config.vapid_key = key.clone();
            }
            config.push_allow_hosts = cli
                .push_allow_hosts
                .iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .collect();
            config.anonymous_devices = cli.dev;
            config.report_difficulty = cli.report_difficulty;
            config.trusted_proxy = cli
                .trusted_proxy
                .map(|Proxy::Cloudflare| TrustedProxy::Cloudflare);
            let hub = Hub::open(config).map_err(|error| error.to_string())?;
            let listener = tokio::net::TcpListener::bind(address)
                .await
                .map_err(|error| format!("{address}: {error}"))?;
            let shutdown = shutdown_signal();
            eprintln!(
                "kelpie-hub listening on http://{address} with data in {}",
                data.display()
            );
            serve(listener, hub, shutdown)
                .await
                .map_err(|error| error.to_string())
        }
    }
}

// The SIGTERM handler is registered before the hub says it listens, so a stop sent right after that line is caught.
fn shutdown_signal() -> impl std::future::Future<Output = ()> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM can be watched");
    async move {
        #[cfg(unix)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kelpie-hub: {message}");
            ExitCode::FAILURE
        }
    }
}
