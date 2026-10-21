use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use kelpie_hub::store::Store;
use kelpie_hub::{Config, Hub, TrustedProxy, database_path, mint_join_code, serve};

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
        (Some(address), _) => Ok(address),
        (None, true) => Ok(DEV_LISTEN.parse().expect("the dev address parses")),
        (None, false) => Err("pass --listen <address>, or --dev for a local hub".to_string()),
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let data = data_dir(&cli)?;
    std::fs::create_dir_all(&data).map_err(|error| format!("{}: {error}", data.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&data, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|error| format!("{}: {error}", data.display()))?;
    match cli.command {
        Some(Command::Admin(Admin::JoinCode)) => {
            let config = Config::new(&data);
            let store = Store::open(&database_path(&data)).map_err(|error| error.to_string())?;
            let code =
                mint_join_code(&store, config.join_code_ttl).map_err(|error| error.to_string())?;
            println!("{code}");
            eprintln!("valid for {} minutes, once", config.join_code_ttl / 60);
            Ok(())
        }
        None => {
            let address = listen_address(&cli)?;
            let mut config = Config::new(&data);
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
