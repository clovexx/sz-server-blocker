use anyhow::Result;
use clap::{Parser, Subcommand};

mod app;
mod firewall;
mod ping;
mod server;
mod settings;
mod sync_api;
mod systemd;
mod ui;

#[derive(Parser)]
#[command(name = "stalzone-server-blocker", about = "Блокировщик серверов Stalzone для Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Ping,
    Apply,
    Clear,
    List {
        #[arg(long)]
        blocked: bool,
    },
    Sync {
        #[arg(long)]
        login: Option<String>,
        #[arg(long)]
        quiet: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => app::run_tui(),
        Some(Commands::Ping) => cmd_ping(),
        Some(Commands::Apply) => cmd_apply(),
        Some(Commands::Clear) => cmd_clear(),
        Some(Commands::List { blocked }) => cmd_list(blocked),
        Some(Commands::Sync { login, quiet }) => cmd_sync(login.as_deref(), quiet),
    }
}

fn cmd_ping() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let servers = server::load_servers()?;
    let results = rt.block_on(ping::ping_servers(&servers));

    for (server, result) in servers.iter().zip(results) {
        println!(
            "{:<16} {:<16} {}",
            server.name,
            server.ip,
            ping::format_ping(Some(result))
        );
    }

    Ok(())
}

fn cmd_apply() -> Result<()> {
    let settings = settings::Settings::load()?;
    let backend = settings.resolve_backend()?;
    let servers = server::load_servers()?;
    let blocked: std::collections::HashSet<_> = settings.blocked.iter().cloned().collect();

    let ips: Vec<String> = servers
        .iter()
        .filter(|s| blocked.contains(&s.name))
        .map(|s| s.ip.clone())
        .collect();

    firewall::apply(backend, &ips)?;
    println!("применено правил: {}", ips.len());
    Ok(())
}

fn cmd_clear() -> Result<()> {
    let settings = settings::Settings::load()?;
    let backend = settings.resolve_backend()?;
    firewall::clear(backend)?;
    println!("блокировка снята");
    Ok(())
}

fn cmd_list(blocked_only: bool) -> Result<()> {
    let settings = settings::Settings::load()?;
    let servers = server::load_servers()?;
    let blocked: std::collections::HashSet<_> = settings.blocked.iter().cloned().collect();

    for server in servers {
        let is_blocked = blocked.contains(&server.name);
        if blocked_only && !is_blocked {
            continue;
        }

        let mark = if is_blocked { "x" } else { " " };
        println!(
            "[{mark}] {:<16} {:<16} {}/{}",
            server.name, server.ip, server.pool, server.region
        );
    }

    Ok(())
}

fn cmd_sync(login: Option<&str>, quiet: bool) -> Result<()> {
    let settings = settings::Settings::load()?;
    let login = login.unwrap_or(&settings.api_login);
    let report = sync_api::sync(login)?;
    if !quiet {
        sync_api::print_report(&report);
    }
    Ok(())
}
