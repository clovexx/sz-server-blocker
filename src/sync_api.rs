use anyhow::{Context, Result};
use serde::Deserialize;

use crate::server::{self, TunnelEntry};

const API_URL: &str = "https://backend.stalcraftx.ru/address_list";

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub fetched: usize,
    pub added: Vec<String>,
    pub updated: Vec<String>,
}

impl SyncReport {
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty()
    }

    pub fn status_message(&self) -> String {
        if !self.has_changes() {
            return "синк: изменений нет".into();
        }

        let mut parts = Vec::new();
        if !self.added.is_empty() {
            parts.push(format!("добавлено {}", self.added.len()));
        }
        if !self.updated.is_empty() {
            parts.push(format!("обновлено {}", self.updated.len()));
        }

        format!("синк: {}", parts.join(", "))
    }
}

#[derive(Deserialize)]
struct AddressList {
    pools: Vec<Pool>,
}

#[derive(Deserialize)]
struct Pool {
    name: String,
    tunnels: Vec<Tunnel>,
}

#[derive(Deserialize)]
struct Tunnel {
    name: String,
    address: String,
}

pub fn sync(login: &str) -> Result<SyncReport> {
    let url = format!(
        "{API_URL}?login={}",
        urlencoding::encode(login)
    );

    let response = ureq::get(&url)
        .call()
        .context("не удалось запросить address_list")?
        .into_string()
        .context("не удалось прочитать ответ address_list")?;

    let list: AddressList =
        serde_json::from_str(&response).context("неверный JSON в ответе address_list")?;

    let mut remote = Vec::new();
    for pool in list.pools {
        let pool_name = pool.name;
        for tunnel in pool.tunnels {
            let (ip, port) = parse_address(&tunnel.address)
                .with_context(|| format!("неверный address у {}", tunnel.name))?;
            remote.push(TunnelEntry {
                name: tunnel.name,
                ip,
                port,
                pool: Some(pool_name.clone()),
            });
        }
    }

    let tunnels_path = server::ensure_user_tunnels()?;
    let mut entries = server::load_tunnel_entries()?;

    let mut report = SyncReport {
        fetched: remote.len(),
        ..Default::default()
    };

    for entry in remote {
        if let Some(existing) = entries.iter_mut().find(|e| e.name == entry.name) {
            let changed = existing.ip != entry.ip
                || existing.port != entry.port
                || existing.pool != entry.pool;
            if changed {
                existing.ip = entry.ip;
                existing.port = entry.port;
                existing.pool = entry.pool;
                report.updated.push(entry.name.clone());
            }
        } else {
            report.added.push(entry.name.clone());
            entries.push(entry);
        }
    }

    if report.has_changes() {
        server::save_tunnel_entries(&tunnels_path, &entries)?;
    }

    Ok(report)
}

fn parse_address(address: &str) -> Result<(String, u16)> {
    let (ip, port) = address
        .rsplit_once(':')
        .with_context(|| format!("ожидался ip:port, получено: {address}"))?;
    let port = port.parse().with_context(|| format!("неверный порт: {port}"))?;
    Ok((ip.to_string(), port))
}

pub fn print_report(report: &SyncReport) {
    println!("серверов в ответе: {}", report.fetched);

    if !report.has_changes() {
        println!("изменений нет");
        return;
    }

    if !report.added.is_empty() {
        println!("добавлено ({}):", report.added.len());
        for name in &report.added {
            println!("  + {name}");
        }
    }

    if !report.updated.is_empty() {
        println!("обновлено ({}):", report.updated.len());
        for name in &report.updated {
            println!("  ~ {name}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_address() {
        let (ip, port) = parse_address("95.213.255.12:29450").unwrap();
        assert_eq!(ip, "95.213.255.12");
        assert_eq!(port, 29450);
    }
}
