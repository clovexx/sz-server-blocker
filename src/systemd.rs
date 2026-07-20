use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_NAME: &str = "stalzone-server-blocker.service";
const UNIT_PATH: &str = "/etc/systemd/system/stalzone-server-blocker.service";

pub fn is_installed() -> bool {
    Path::new(UNIT_PATH).is_file()
}

pub fn is_enabled() -> bool {
    Command::new("systemctl")
        .args(["is-enabled", "--quiet", SERVICE_NAME])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn status_label() -> String {
    if !is_installed() {
        return "не установлен".into();
    }
    if is_enabled() {
        "включён".into()
    } else {
        "установлен, выкл.".into()
    }
}

pub fn install_unit(binary: &Path) -> Result<()> {
    crate::firewall::ensure_root()?;

    let content = format!(
        "[Unit]\n\
         Description=Stalzone server blocker\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         RemainAfterExit=yes\n\
         ExecStart={} apply\n\
         ExecStop={} clear\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        binary.display(),
        binary.display()
    );

    fs::write(UNIT_PATH, content).context("не удалось записать unit systemd")?;
    run_systemctl(&["daemon-reload"])?;
    Ok(())
}

pub fn enable() -> Result<()> {
    crate::firewall::ensure_root()?;
    run_systemctl(&["enable", "--now", SERVICE_NAME])
}

pub fn disable() -> Result<()> {
    crate::firewall::ensure_root()?;
    let _ = run_systemctl(&["disable", "--now", SERVICE_NAME]);
    Ok(())
}

pub fn sync_boot(apply_on_boot: bool, binary: &Path) -> Result<()> {
    if apply_on_boot {
        if !is_installed() {
            install_unit(binary)?;
        }
        enable()
    } else if is_installed() {
        disable()
    } else {
        Ok(())
    }
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .context("не удалось запустить systemctl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("systemctl {} завершился с ошибкой: {}", args.join(" "), stderr.trim());
    }

    Ok(())
}

pub fn binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("не удалось определить путь к бинарнику")
}
