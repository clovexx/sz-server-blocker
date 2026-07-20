use super::{CHAIN_NAME, PORT_END, PORT_START, run};
use anyhow::{Context, Result};
use std::process::Command;

pub fn apply(ips: &[String]) -> Result<()> {
    clear()?;

    if ips.is_empty() {
        return Ok(());
    }

    ensure_chain()?;
    run(
        "iptables",
        &["-C", "OUTPUT", "-j", CHAIN_NAME],
    )
    .or_else(|_| run("iptables", &["-I", "OUTPUT", "-j", CHAIN_NAME]))?;

    let mark = format!("{:#x}", super::PING_SK_MARK);
    run(
        "iptables",
        &["-A", CHAIN_NAME, "-m", "mark", "--mark", &mark, "-j", "ACCEPT"],
    )?;

    let port_range = format!("{PORT_START}:{PORT_END}");
    for ip in ips {
        for proto in ["tcp", "udp"] {
            run(
                "iptables",
                &[
                    "-A",
                    CHAIN_NAME,
                    "-d",
                    ip,
                    "-p",
                    proto,
                    "--dport",
                    &port_range,
                    "-j",
                    "DROP",
                ],
            )?;
        }
    }

    Ok(())
}

pub fn clear() -> Result<()> {
    if !is_active()? {
        return Ok(());
    }

    let _ = Command::new("iptables")
        .args(["-D", "OUTPUT", "-j", CHAIN_NAME])
        .output();
    let _ = Command::new("iptables")
        .args(["-F", CHAIN_NAME])
        .output();
    let _ = Command::new("iptables")
        .args(["-X", CHAIN_NAME])
        .output();

    Ok(())
}

pub fn is_active() -> Result<bool> {
    let output = Command::new("iptables")
        .args(["-L", CHAIN_NAME])
        .output()
        .context("не удалось запустить iptables")?;

    Ok(output.status.success())
}

fn ensure_chain() -> Result<()> {
    if is_active()? {
        run("iptables", &["-F", CHAIN_NAME]).map(|_| ())
    } else {
        run("iptables", &["-N", CHAIN_NAME]).map(|_| ())
    }
}
