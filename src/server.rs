use anyhow::{Context, Result, bail};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 29450;
const EMBEDDED_TUNNELS: &str = include_str!("../tunnels.txt");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelEntry {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub pool: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub pool: String,
    pub region: String,
}

pub fn load_servers() -> Result<Vec<Server>> {
    Ok(load_servers_from_entries(load_tunnel_entries()?))
}

pub fn load_tunnel_entries() -> Result<Vec<TunnelEntry>> {
    let content = read_tunnels_content()?;
    parse_tunnel_entries(&content)
}

pub fn tunnels_config_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "stalzone-server-blocker")
        .context("не удалось определить каталог конфигурации")?;
    Ok(dirs.config_dir().join("tunnels.txt"))
}

pub fn ensure_user_tunnels() -> Result<PathBuf> {
    let path = tunnels_config_path()?;
    if path.is_file() {
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    if let Some(existing) = find_tunnels_file() {
        if existing != path {
            fs::copy(&existing, &path)?;
            return Ok(path);
        }
    }

    fs::write(&path, EMBEDDED_TUNNELS)?;
    Ok(path)
}

pub fn save_tunnel_entries(path: &Path, entries: &[TunnelEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let lines: Vec<String> = entries.iter().map(format_tunnel_line).collect();

    let content = lines.join("\n");
    let content = if content.is_empty() {
        content
    } else {
        format!("{content}\n")
    };
    fs::write(path, content).with_context(|| format!("не удалось записать {}", path.display()))?;
    Ok(())
}

fn format_tunnel_line(entry: &TunnelEntry) -> String {
    let base = if entry.port == DEFAULT_PORT {
        format!("{} - {}", entry.name, entry.ip)
    } else {
        format!("{} - {}:{}", entry.name, entry.ip, entry.port)
    };
    match &entry.pool {
        Some(pool) => format!("{base} @{pool}"),
        None => base,
    }
}

fn read_tunnels_content() -> Result<String> {
    if let Some(path) = find_tunnels_file() {
        return fs::read_to_string(&path)
            .with_context(|| format!("не удалось прочитать {}", path.display()));
    }
    Ok(EMBEDDED_TUNNELS.to_string())
}

fn find_tunnels_file() -> Option<PathBuf> {
    if let Some(config_dir) = directories::ProjectDirs::from("", "", "stalzone-server-blocker") {
        let path = config_dir.config_dir().join("tunnels.txt");
        if path.is_file() {
            return Some(path);
        }
    }

    let cwd = Path::new("tunnels.txt");
    if cwd.is_file() {
        return Some(cwd.to_path_buf());
    }

    None
}

pub fn parse_tunnel_entries(content: &str) -> Result<Vec<TunnelEntry>> {
    let re = Regex::new(r"^(.+?)\s*-\s*(\d+\.\d+\.\d+\.\d+)(?::(\d+))?(?:\s+@(.+))?\s*$")?;
    let mut entries = Vec::new();

    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let caps = re
            .captures(line)
            .with_context(|| format!("неверная строка в tunnels.txt, {}: {line}", line_no + 1))?;

        let name = caps[1].trim().to_string();
        let ip = caps[2].to_string();
        let port = caps
            .get(3)
            .map(|m| m.as_str().parse::<u16>())
            .transpose()
            .with_context(|| format!("неверный порт в строке {}", line_no + 1))?
            .unwrap_or(DEFAULT_PORT);
        let pool = caps.get(4).map(|m| m.as_str().trim().to_string());

        entries.push(TunnelEntry {
            name,
            ip,
            port,
            pool,
        });
    }

    if entries.is_empty() {
        bail!("в tunnels.txt не найдено серверов");
    }

    Ok(entries)
}

fn load_servers_from_entries(entries: Vec<TunnelEntry>) -> Vec<Server> {
    let mut servers: Vec<Server> = entries
        .into_iter()
        .map(|entry| {
            let (inferred_pool, region) = infer_pool_region(&entry.name);
            let pool = entry.pool.unwrap_or(inferred_pool);
            Server {
                name: entry.name,
                ip: entry.ip,
                port: entry.port,
                pool,
                region,
            }
        })
        .collect();
    servers.sort_by(|a, b| a.pool.cmp(&b.pool).then_with(|| a.name.cmp(&b.name)));
    servers
}

fn infer_pool_region(name: &str) -> (String, String) {
    if name.starts_with("GAME-EU") {
        return ("GAME-EU".into(), "EU".into());
    }
    if name.starts_with("GAME-NA") {
        return ("GAME-NA".into(), "NA".into());
    }
    if name.starts_with("GAME-SEA") {
        return ("GAME-SEA".into(), "SEA".into());
    }
    if name.starts_with("WAW2") {
        return ("WAW2".into(), "EU".into());
    }
    if name.starts_with("WAW") {
        return ("WAW".into(), "EU".into());
    }
    if name.starts_with("NYC") {
        return ("NYC".into(), "NA".into());
    }
    if name.starts_with("LAX") {
        return ("LAX".into(), "NA".into());
    }
    if name.starts_with("SYD") {
        return ("SYD".into(), "SEA".into());
    }

    let prefixes = [
        ("MSK2", "RU"),
        ("MSK1", "RU"),
        ("EKB", "RU"),
        ("SMR", "RU"),
        ("RST", "RU"),
        ("NSK", "RU"),
        ("KRY", "RU"),
        ("KHB", "RU"),
        ("AST", "RU"),
        ("RIG", "RU"),
        ("VIL", "RU"),
        ("MNK", "RU"),
        ("TAS", "RU"),
    ];

    for (prefix, region) in prefixes {
        if name.starts_with(prefix) {
            return (prefix.to_string(), region.to_string());
        }
    }

    let pool = name
        .rsplit_once('-')
        .map(|(left, _)| left.to_string())
        .unwrap_or_else(|| name.to_string());

    (pool, "UNK".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tunnel_line() {
        let entries = parse_tunnel_entries("MSK2-1 - 95.213.255.12\n").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "MSK2-1");
        assert_eq!(entries[0].ip, "95.213.255.12");
        assert_eq!(entries[0].port, DEFAULT_PORT);

        let servers = load_servers_from_entries(entries);
        assert_eq!(servers[0].pool, "MSK2");
        assert_eq!(servers[0].region, "RU");
    }

    #[test]
    fn parses_tunnel_line_with_pool() {
        let entries =
            parse_tunnel_entries("SRVMGR-ACCESSOR-OFT-RU-7A - 176.114.88.147 @CLOUD-RU-7\n").unwrap();
        assert_eq!(entries[0].pool.as_deref(), Some("CLOUD-RU-7"));

        let servers = load_servers_from_entries(entries);
        assert_eq!(servers[0].pool, "CLOUD-RU-7");
    }

    #[test]
    fn sorts_by_pool_then_name() {
        let entries = parse_tunnel_entries(
            "MSK2-2 - 1.1.1.1 @MSK2\nMSK1-1 - 2.2.2.2 @MSK1\nMSK2-1 - 3.3.3.3 @MSK2\n",
        )
        .unwrap();
        let servers = load_servers_from_entries(entries);
        assert_eq!(
            servers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["MSK1-1", "MSK2-1", "MSK2-2"]
        );
    }

    #[test]
    fn loads_embedded_tunnels() {
        let servers = load_servers().unwrap();
        assert!(servers.len() >= 50);
    }
}
