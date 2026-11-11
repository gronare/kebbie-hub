use std::process::ExitCode;

use axum::Router;
use clap::{Parser, Subcommand};
use kebbie_hub::Hub;
use kebbie_hub::cli::{Admin, HubArgs, exit_code, run_admin, serve_until_stopped};

#[derive(Parser)]
#[command(
    name = "kebbie-hub",
    version,
    about = "The kebbie hub: relays sealed boards and commands between machines and your account"
)]
struct Cli {
    #[command(flatten)]
    hub: HubArgs,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(subcommand, about = "Operator tasks against the hub's data directory")]
    Admin(Admin),
}

async fn run(cli: Cli) -> Result<(), String> {
    let data = cli.hub.data_dir()?;
    match &cli.command {
        Some(Command::Admin(admin)) => run_admin(&data, admin),
        None => {
            let address = cli.hub.listen_address()?;
            let hub = Hub::open(cli.hub.config(&data)?).map_err(|error| error.to_string())?;
            serve_until_stopped(address, hub, Router::new(), &data).await
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    exit_code(run(Cli::parse()).await)
}
