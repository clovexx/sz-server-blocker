use super::{PORT_END, PORT_START, TABLE_NAME};
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::Command;

pub fn apply(ips: &[String]) -> Result<()> {
    clear()?;

    if ips.is_empty() {
        return Ok(());
    }

    let mut script = String::from("table inet ");
    script.push_str(TABLE_NAME);
    script.push_str(" {\n chain output {\n");
    script.push_str("  type filter hook output priority 0; policy accept;\n");
    script.push_str(&format!(
        "  meta mark {:#x} accept\n",
        super::PING_SK_MARK
    ));

    for ip in ips {
        script.push_str(&format!(
            "  ip daddr {ip} tcp dport {PORT_START}-{PORT_END} drop\n"
        ));
        script.push_str(&format!(
            "  ip daddr {ip} udp dport {PORT_START}-{PORT_END} drop\n"
        ));
    }

    script.push_str(" }\n}\n");
    run_script(&script)
}

pub fn clear() -> Result<()> {
    if !is_active()? {
        return Ok(());
    }

    let output = Command::new("nft")
        .args(["delete", "table", "inet", TABLE_NAME])
        .output()
        .context("не удалось запустить nft")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("не удалось удалить таблицу nft: {}", stderr.trim());
    }

    Ok(())
}

pub fn is_active() -> Result<bool> {
    let output = Command::new("nft")
        .args(["list", "table", "inet", TABLE_NAME])
        .output()?;

    Ok(output.status.success())
}

fn run_script(script: &str) -> Result<()> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("не удалось запустить nft")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .context("не удалось передать правила в nft")?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("не удалось применить правила nft: {}", stderr.trim());
    }

    Ok(())
}
