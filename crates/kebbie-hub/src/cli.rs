use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use axum::Router;
use clap::{Args, Subcommand, ValueEnum};
use kebbie_hub_api::SignupMode;
use kebbie_hub_protocol::Id;

use crate::store::{MadeOwner, Store};
use crate::{
    AccountSwitch, Config, DEV_ORIGIN, DEV_RP_ID, DEV_VAPID_SUBJECT, GITHUB_API_URL,
    GITHUB_WEB_URL, Hub, Secret, TrustedProxy, database_path, mint_invite, mint_join_code, now,
    serve_with, set_account_active,
};

const DEV_LISTEN: &str = "127.0.0.1:8470";
const GITHUB_SECRET_ENV: &str = "KEBBIE_HUB_GITHUB_CLIENT_SECRET";

#[derive(Args)]
pub struct HubArgs {
    #[arg(
        long,
        global = true,
        help = "Use localhost and a data directory under the system temp directory"
    )]
    pub dev: bool,
    #[arg(long, global = true, help = "Directory holding the hub's sqlite file")]
    pub data: Option<PathBuf>,
    #[arg(long, help = "Address to listen on")]
    pub listen: Option<SocketAddr>,
    #[arg(
        long,
        value_enum,
        help = "Take the client address from this proxy's header instead of the TCP peer"
    )]
    pub trusted_proxy: Option<Proxy>,
    #[arg(
        long,
        help = "Passkey relying party id: the hub's domain, like hub.example.com"
    )]
    pub rp_id: Option<String>,
    #[arg(
        long,
        help = "The origin people open the hub at, like https://hub.example.com"
    )]
    pub origin: Option<String>,
    #[arg(
        long,
        help = "Contact the push services see in every VAPID token: mailto:you@example.com or an https: URL [default: the origin]"
    )]
    pub vapid_subject: Option<String>,
    #[arg(
        long,
        help = "File holding the hub's VAPID private key, created on first start [default: <data>/vapid.key]"
    )]
    pub vapid_key: Option<PathBuf>,
    #[arg(
        long = "push-allow-host",
        value_name = "HOST",
        help = "Accept push endpoints on this host besides the known push services; *.example.com matches subdomains, and a loopback host may use http"
    )]
    pub push_allow_hosts: Vec<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = Signup::Invite,
        help = "Who signs up: people with an invite, or anyone whose machine opened a connect link"
    )]
    pub signup: Signup,
    #[arg(
        long,
        value_enum,
        default_value_t = Switch::Off,
        help = "With on, a device that allows changes answers questions and approves pushes through the hub, signed by its account; with off, the hub refuses commands and the web hides them"
    )]
    pub commands: Switch,
    #[arg(
        long,
        value_name = "ID",
        help = "Client id of the GitHub OAuth app people sign in with; its secret comes from KEBBIE_HUB_GITHUB_CLIENT_SECRET"
    )]
    pub github_client_id: Option<String>,
    #[arg(
        long,
        value_name = "URL",
        default_value = GITHUB_API_URL,
        help = "Where the hub reads the GitHub user, for GitHub Enterprise or a test double"
    )]
    pub github_api_url: String,
    #[arg(
        long,
        value_name = "URL",
        default_value = GITHUB_WEB_URL,
        help = "Where people approve the hub on GitHub and the hub trades the code for a token"
    )]
    pub github_web_url: String,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Signup {
    Invite,
    Open,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    Off,
    On,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Proxy {
    Cloudflare,
}

#[derive(Subcommand)]
pub enum Admin {
    #[command(about = "Mint a one-time code a machine joins with")]
    JoinCode,
    #[command(about = "Mint a one-time code a person signs up with")]
    Invite,
    #[command(about = "List the machines that joined")]
    Machines,
    #[command(about = "Remove a machine, its board, queue and admissions")]
    RemoveMachine { machine: String },
    #[command(about = "List the accounts by id, with role, machines and when they were last seen")]
    Accounts,
    #[command(about = "Make an account the hub's owner, who cannot be deactivated")]
    Owner {
        #[arg(help = "The account's id (acc_…), as admin accounts lists it")]
        account: String,
        #[arg(
            long,
            help = "Demote the current owner to member; without it an existing owner is kept and nothing changes"
        )]
        replace: bool,
    },
    #[command(
        about = "Deactivate an account: its sessions end and its machines are refused until it is activated"
    )]
    Deactivate {
        #[arg(help = "The account's id (acc_…), as admin accounts lists it")]
        account: String,
    },
    #[command(about = "Activate a deactivated account again")]
    Activate {
        #[arg(help = "The account's id (acc_…), as admin accounts lists it")]
        account: String,
    },
}

impl HubArgs {
    pub fn data_dir(&self) -> Result<PathBuf, String> {
        let data = match (&self.data, self.dev) {
            (Some(dir), _) => dir.clone(),
            (None, true) => std::env::temp_dir().join("kebbie-hub-dev"),
            (None, false) => return Err("pass --data <dir>, or --dev for a local hub".to_string()),
        };
        std::fs::create_dir_all(&data).map_err(|error| format!("{}: {error}", data.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(&data, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .map_err(|error| format!("{}: {error}", data.display()))?;
        Ok(data)
    }

    pub fn listen_address(&self) -> Result<SocketAddr, String> {
        match (self.listen, self.dev) {
            (Some(address), true) if !address.ip().is_loopback() => Err(format!(
                "--dev serves plain http with test defaults; listen on a loopback address, not {address}"
            )),
            (Some(address), _) => Ok(address),
            (None, true) => Ok(DEV_LISTEN.parse().expect("the dev address parses")),
            (None, false) => Err("pass --listen <address>, or --dev for a local hub".to_string()),
        }
    }

    fn relying_party(&self) -> Result<(String, String), String> {
        match (&self.rp_id, &self.origin, self.dev) {
            (Some(_), Some(origin), false) if !origin.starts_with("https://") => Err(format!(
                "--origin must be https:// unless --dev, not {origin}"
            )),
            (Some(rp_id), Some(origin), _) => Ok((rp_id.clone(), origin.clone())),
            (None, None, true) => Ok((DEV_RP_ID.to_string(), DEV_ORIGIN.to_string())),
            _ => Err(
                "pass --rp-id <domain> and --origin <url>, or --dev for a local hub".to_string(),
            ),
        }
    }

    fn github_secret(&self) -> Result<Option<Secret>, String> {
        if self.github_client_id.is_none() {
            return Ok(None);
        }
        match std::env::var(GITHUB_SECRET_ENV) {
            Ok(secret) if !secret.trim().is_empty() => Ok(Some(Secret::new(secret.trim()))),
            _ => Err(format!(
                "--github-client-id needs the app's client secret in {GITHUB_SECRET_ENV}"
            )),
        }
    }

    pub fn config(&self, data: &Path) -> Result<Config, String> {
        let (rp_id, origin) = self.relying_party()?;
        let mut config = Config::new(data);
        config.vapid_subject = match &self.vapid_subject {
            Some(subject) => subject.clone(),
            None if origin.starts_with("https://") => origin.clone(),
            None => DEV_VAPID_SUBJECT.to_string(),
        };
        if let Some(key) = &self.vapid_key {
            config.vapid_key = key.clone();
        }
        config.push_allow_hosts = self.push_allow_hosts.iter().map(|host| host.trim().to_ascii_lowercase()).collect();
        config.rp_id = rp_id;
        config.origin = origin;
        config.trusted_proxy = self
            .trusted_proxy
            .map(|Proxy::Cloudflare| TrustedProxy::Cloudflare);
        config.signup = match self.signup {
            Signup::Invite => SignupMode::Invite,
            Signup::Open => SignupMode::Open,
        };
        config.commands = self.commands == Switch::On;
        config.github_client_secret = self.github_secret()?;
        config.github_client_id = self.github_client_id.clone();
        config.github_api_url = self.github_api_url.clone();
        config.github_web_url = self.github_web_url.clone();
        Ok(config)
    }
}

pub fn open_store(data: &Path) -> Result<Store, String> {
    Store::open(&database_path(data)).map_err(|error| error.to_string())
}

fn account_id(account: &str) -> Result<&str, String> {
    let account = account.trim();
    if account.starts_with("acc_") {
        Ok(account)
    } else {
        Err(format!(
            "{account} is not an account id; kebbie-hub admin accounts lists them (acc_…)"
        ))
    }
}

fn make_owner(data: &Path, account: &str, replace: bool) -> Result<(), String> {
    let account = account_id(account)?;
    let store = open_store(data)?;
    match store
        .make_owner(account, replace, now())
        .map_err(|error| error.to_string())?
    {
        MadeOwner::Made { replaced } if replaced.is_empty() => println!("{account} is the owner"),
        MadeOwner::Made { replaced } => println!(
            "{account} is the owner; {} is a member now",
            replaced.join(", ")
        ),
        MadeOwner::AlreadyOwner => println!("{account} is the owner already"),
        MadeOwner::OtherOwner(owners) => {
            return Err(format!(
                "{} is the owner; add --replace to make {account} the owner and {} a member",
                owners.join(", "),
                owners.join(", ")
            ));
        }
        MadeOwner::Deactivated => {
            return Err(format!(
                "{account} is deactivated; activate it before making it the owner"
            ));
        }
        MadeOwner::NoAccount => return Err(format!("no account {account}")),
    }
    Ok(())
}

fn switch_account(data: &Path, account: &str, active: bool) -> Result<(), String> {
    let account = account_id(account)?;
    let store = open_store(data)?;
    match set_account_active(&store, account, active, "cli").map_err(|error| error.to_string())? {
        AccountSwitch::Switched if active => println!("{account} is active"),
        AccountSwitch::Switched => {
            println!("{account} is deactivated; its sessions ended and its machines are refused")
        }
        AccountSwitch::Owner => {
            return Err(format!("{account} is the owner, who cannot be deactivated"));
        }
        AccountSwitch::NoAccount => return Err(format!("no account {account}")),
    }
    Ok(())
}

pub fn run_admin(data: &Path, admin: &Admin) -> Result<(), String> {
    let defaults = Config::new(data);
    let failed = |error: rusqlite::Error| error.to_string();
    match admin {
        Admin::JoinCode => {
            let code = mint_join_code(&open_store(data)?, defaults.join_code_ttl).map_err(failed)?;
            println!("{code}");
            eprintln!("valid for {} minutes, once", defaults.join_code_ttl / 60);
        }
        Admin::Invite => {
            let code = mint_invite(&open_store(data)?, defaults.invite_ttl).map_err(failed)?;
            println!("{code}");
            eprintln!("valid for {} days, once", defaults.invite_ttl / 86_400);
        }
        Admin::Machines => {
            for machine in open_store(data)?.machines().map_err(failed)? {
                let seen = machine
                    .last_seen
                    .map_or("-".to_string(), |at| at.to_string());
                println!(
                    "{} {} joined {} seen {seen}",
                    machine.id, machine.name, machine.joined_at
                );
            }
        }
        Admin::Owner { account, replace } => make_owner(data, account, *replace)?,
        Admin::Deactivate { account } => switch_account(data, account, false)?,
        Admin::Activate { account } => switch_account(data, account, true)?,
        Admin::Accounts => {
            for row in open_store(data)?.admin_accounts().map_err(failed)? {
                let role = if row.account.owner { "owner" } else { "member" };
                let seen = row.last_seen.map_or("-".to_string(), |at| at.to_string());
                let off = if row.account.deactivated_at.is_some() {
                    " deactivated"
                } else {
                    ""
                };
                println!(
                    "{} {role}{off} created {} machines {} seen {seen}",
                    row.account.id, row.account.created_at, row.machines
                );
            }
        }
        Admin::RemoveMachine { machine } => {
            let machine = Id::parse(machine).map_err(|error| error.to_string())?;
            let store = open_store(data)?;
            if !store.remove_machine(&machine).map_err(failed)? {
                return Err(format!("no machine {machine}"));
            }
            store
                .log(now(), "machine.removed", Some(&machine), None)
                .map_err(failed)?;
            println!("removed {machine}");
        }
    }
    Ok(())
}

pub async fn serve_until_stopped(
    address: SocketAddr,
    hub: Arc<Hub>,
    extra: Router<Arc<Hub>>,
    data: &Path,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("{address}: {error}"))?;
    let shutdown = shutdown_signal();
    eprintln!(
        "kebbie-hub listening on http://{address} with data in {}",
        data.display()
    );
    if let Some(url) = hub.setup_url() {
        eprintln!("kebbie-hub: no account yet. Create the owner account, once, at:\n\n    {url}\n");
    }
    serve_with(listener, hub, extra, shutdown)
        .await
        .map_err(|error| error.to_string())
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

pub fn exit_code(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kebbie-hub: {message}");
            ExitCode::FAILURE
        }
    }
}
