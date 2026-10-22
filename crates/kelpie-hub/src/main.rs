use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use kelpie_hub::store::Store;
use kelpie_hub::{
    Config, DEV_ORIGIN, DEV_RP_ID, Hub, TrustedProxy, database_path, mint_invite, mint_join_code,
    serve,
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
            config.anonymous_devices = cli.dev;
            config.trusted_proxy = cli
                .trusted_proxy
                .map(|Proxy::Cloudflare| TrustedProxy::Cloudflare);
            let hub = Hub::open(config).map_err(|error| error.to_string())?;
            let listener = tokio::net::TcpListener::bind(address)
                .await
                .map_err(|error| format!("{address}: {error}"))?;
            eprintln!(
                "kelpie-hub listening on http://{address} with data in {}",
                data.display()
            );
            let shutdown = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            serve(listener, hub, shutdown)
                .await
                .map_err(|error| error.to_string())
        }
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
