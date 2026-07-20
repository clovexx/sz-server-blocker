use anyhow::{Context, Result, bail};
use std::process::{Command, Output, Stdio};

pub const TABLE_NAME: &str = "stalzone_blocker";
pub const CHAIN_NAME: &str = "STALZONE_BLOCKER";
pub const PORT_START: u16 = 29450;
pub const PORT_END: u16 = 29460;
pub const PING_SK_MARK: u32 = 0x535a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Nftables,
    Iptables,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nftables => "nftables",
            Self::Iptables => "iptables",
        }
    }
}

pub fn detect_backend() -> Result<Backend> {
    if command_ok("nft", &["--version"]) {
        return Ok(Backend::Nftables);
    }
    if command_ok("iptables", &["--version"]) {
        return Ok(Backend::Iptables);
    }
    bail!("в PATH не найдены nft и iptables");
}

pub fn apply(backend: Backend, ips: &[String]) -> Result<()> {
    ensure_root()?;
    let ips = dedupe_ips(ips);
    match backend {
        Backend::Nftables => nftables::apply(&ips),
        Backend::Iptables => iptables::apply(&ips),
    }
}

pub fn clear(backend: Backend) -> Result<()> {
    ensure_root()?;
    match backend {
        Backend::Nftables => nftables::clear(),
        Backend::Iptables => iptables::clear(),
    }
}

pub fn is_active(backend: Backend) -> Result<bool> {
    match backend {
        Backend::Nftables => nftables::is_active(),
        Backend::Iptables => iptables::is_active(),
    }
}

pub fn ensure_root() -> Result<()> {
    if !is_root() {
        bail!("нужны права root — запустите: sudo stalzone-server-blocker");
    }
    Ok(())
}

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn command_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run(program: &str, args: &[&str]) -> Result<Output> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("не удалось запустить {program}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} завершился с ошибкой: {}", stderr.trim());
    }

    Ok(output)
}

fn dedupe_ips(ips: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for ip in ips {
        if !out.iter().any(|x| x == ip) {
            out.push(ip.clone());
        }
    }
    out
}

mod iptables;
mod nftables;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_backend() {
        let _ = detect_backend();
    }
}
