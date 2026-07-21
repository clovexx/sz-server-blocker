use std::collections::HashSet;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::TableState;
use ratatui::Terminal;
use std::io::stdout;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;

use crate::firewall::{self, Backend};
use crate::ping::{self, PingResult};
use crate::server::{self, Server};
use crate::settings::Settings;
use crate::sync_api;
use crate::systemd;
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Servers,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
    ApiLogin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRow {
    Backend = 0,
    Boot = 1,
    Systemd = 2,
    SyncOnStartup = 3,
    ApiLogin = 4,
}

const SETTINGS_ROWS: usize = 5;

#[derive(Debug, Clone)]
pub enum StatusMessage {
    Info(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ServerEntry {
    pub server: Server,
    pub blocked: bool,
    pub ping: Option<PingResult>,
}

struct PingJob {
    total: usize,
    completed: usize,
    rx: mpsc::UnboundedReceiver<(usize, PingResult)>,
}

pub struct App {
    pub screen: Screen,
    pub entries: Vec<ServerEntry>,
    pub list_index: usize,
    pub settings_index: usize,
    pub filter: String,
    pub input_mode: InputMode,
    pub backend: Backend,
    pub blocking_active: bool,
    pub pinging: bool,
    ping_job: Option<PingJob>,
    pub status_message: Option<StatusMessage>,
    pub settings: Settings,
    pub systemd_status: String,
    pub api_login_draft: String,
    pub table_state: TableState,
    pub settings_table_state: TableState,
    rt: Runtime,
}

impl App {
    pub fn new() -> Result<Self> {
        let settings = Settings::load()?;
        let backend = settings.resolve_backend()?;
        let status_message = if settings.sync_on_startup {
            Self::run_sync(&settings).ok().filter(|report| report.has_changes()).map(|report| {
                StatusMessage::Info(report.status_message())
            })
        } else {
            None
        };
        let servers = server::load_servers()?;
        let blocked_set: std::collections::HashSet<_> =
            settings.blocked.iter().cloned().collect();

        let entries = servers
            .into_iter()
            .map(|server| {
                let blocked = blocked_set.contains(&server.name);
                ServerEntry {
                    server,
                    blocked,
                    ping: None,
                }
            })
            .collect();

        let blocking_active = firewall::is_active(backend).unwrap_or(false);
        let systemd_status = systemd::status_label();

        Ok(Self {
            screen: Screen::Servers,
            entries,
            list_index: 0,
            settings_index: 0,
            filter: String::new(),
            input_mode: InputMode::Normal,
            backend,
            blocking_active,
            pinging: false,
            ping_job: None,
            status_message,
            settings,
            systemd_status,
            api_login_draft: String::new(),
            table_state: TableState::default(),
            settings_table_state: TableState::default(),
            rt: Runtime::new().context("не удалось создать tokio runtime")?,
        })
    }

    fn run_sync(settings: &Settings) -> Result<sync_api::SyncReport> {
        sync_api::sync(&settings.api_login)
    }

    pub fn sync_servers(&mut self) -> Result<()> {
        let report = Self::run_sync(&self.settings)?;
        if report.has_changes() {
            self.reload_servers()?;
        }
        self.status_message = Some(StatusMessage::Info(report.status_message()));
        Ok(())
    }

    fn reload_servers(&mut self) -> Result<()> {
        let blocked: HashSet<_> = self
            .entries
            .iter()
            .filter(|e| e.blocked)
            .map(|e| e.server.name.clone())
            .collect();
        let pings: std::collections::HashMap<_, _> = self
            .entries
            .iter()
            .filter_map(|e| e.ping.map(|ping| (e.server.name.clone(), ping)))
            .collect();

        let servers = server::load_servers()?;
        self.entries = servers
            .into_iter()
            .map(|server| {
                let name = server.name.clone();
                ServerEntry {
                    blocked: blocked.contains(&name),
                    ping: pings.get(&name).copied(),
                    server,
                }
            })
            .collect();

        if self.list_index >= self.visible_indices().len().max(1) {
            self.list_index = 0;
        }
        self.reset_list_scroll();

        Ok(())
    }

    pub fn reset_list_scroll(&mut self) {
        *self.table_state.offset_mut() = 0;
        self.table_state.select(Some(self.list_index));
    }

    pub fn open_settings(&mut self) {
        self.screen = Screen::Settings;
        self.settings_index = 0;
        self.input_mode = InputMode::Normal;
        self.systemd_status = systemd::status_label();
        self.status_message = None;
    }

    pub fn close_settings(&mut self) {
        self.screen = Screen::Servers;
        self.input_mode = InputMode::Normal;
        self.status_message = None;
    }

    pub fn move_settings_selection(&mut self, delta: i32) {
        if self.input_mode == InputMode::ApiLogin {
            return;
        }
        let next = self.settings_index as i32 + delta;
        self.settings_index = next.clamp(0, SETTINGS_ROWS as i32 - 1) as usize;
    }

    pub fn cycle_backend(&mut self, forward: bool) -> Result<()> {
        self.settings.backend = if forward {
            self.settings.backend.next()
        } else {
            self.settings.backend.prev()
        };
        self.settings.save()?;
        self.backend = self.settings.resolve_backend()?;
        self.blocking_active = firewall::is_active(self.backend).unwrap_or(false);
        self.status_message = Some(StatusMessage::Info(format!(
            "бэкенд: {}",
            self.settings.backend.label()
        )));
        Ok(())
    }

    pub fn toggle_apply_on_boot(&mut self) -> Result<()> {
        self.settings.apply_on_boot = !self.settings.apply_on_boot;
        self.settings.save()?;

        let binary = systemd::binary_path()?;
        systemd::sync_boot(self.settings.apply_on_boot, &binary)?;
        self.systemd_status = systemd::status_label();

        let msg = if self.settings.apply_on_boot {
            "автоприменение при загрузке включено"
        } else {
            "автоприменение при загрузке выключено"
        };
        self.status_message = Some(StatusMessage::Info(msg.into()));
        Ok(())
    }

    pub fn toggle_sync_on_startup(&mut self) -> Result<()> {
        self.settings.sync_on_startup = !self.settings.sync_on_startup;
        self.settings.save()?;

        let msg = if self.settings.sync_on_startup {
            "автосинк при старте включён"
        } else {
            "автосинк при старте выключен"
        };
        self.status_message = Some(StatusMessage::Info(msg.into()));
        Ok(())
    }

    pub fn start_api_login_edit(&mut self) {
        self.api_login_draft = self.settings.api_login.clone();
        self.input_mode = InputMode::ApiLogin;
        self.status_message = Some(StatusMessage::Info("логин для address_list".into()));
    }

    pub fn save_api_login(&mut self) -> Result<()> {
        let trimmed = self.api_login_draft.trim();
        if trimmed.is_empty() {
            anyhow::bail!("логин не может быть пустым");
        }

        self.settings.api_login = trimmed.to_string();
        self.settings.save()?;
        self.input_mode = InputMode::Normal;
        self.status_message = Some(StatusMessage::Info(format!(
            "логин API: {}",
            self.settings.api_login
        )));
        Ok(())
    }

    pub fn cancel_api_login_edit(&mut self) {
        self.input_mode = InputMode::Normal;
        self.api_login_draft.clear();
        self.status_message = None;
    }

    pub fn install_systemd(&mut self) -> Result<()> {
        let binary = systemd::binary_path()?;
        systemd::install_unit(&binary)?;

        if self.settings.apply_on_boot {
            systemd::enable()?;
        }

        self.systemd_status = systemd::status_label();
        self.status_message = Some(StatusMessage::Info("unit systemd установлен".into()));
        Ok(())
    }

    pub fn activate_settings_row(&mut self) -> Result<()> {
        match SettingsRow::from_index(self.settings_index) {
            Some(SettingsRow::Backend) => self.cycle_backend(true)?,
            Some(SettingsRow::Boot) => self.toggle_apply_on_boot()?,
            Some(SettingsRow::Systemd) => self.install_systemd()?,
            Some(SettingsRow::SyncOnStartup) => self.toggle_sync_on_startup()?,
            Some(SettingsRow::ApiLogin) => self.start_api_login_edit(),
            None => {}
        }
        Ok(())
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.entries.len()).collect();
        }

        let needle = self.filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.server.name.to_lowercase().contains(&needle)
                    || entry.server.ip.contains(&needle)
                    || entry.server.pool.to_lowercase().contains(&needle)
                    || entry.server.region.to_lowercase().contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn selected_server_index(&self) -> Option<usize> {
        let visible = self.visible_indices();
        visible.get(self.list_index).copied()
    }

    pub fn move_selection(&mut self, delta: i32) {
        let visible_len = self.visible_indices().len();
        if visible_len == 0 {
            self.list_index = 0;
            return;
        }

        let next = self.list_index as i32 + delta;
        self.list_index = next.clamp(0, visible_len as i32 - 1) as usize;
    }

    pub fn toggle_selected(&mut self) {
        if let Some(idx) = self.selected_server_index() {
            self.entries[idx].blocked = !self.entries[idx].blocked;
            self.status_message = None;
        }
    }

    pub fn toggle_pool(&mut self) {
        let Some(idx) = self.selected_server_index() else {
            return;
        };

        let pool = self.entries[idx].server.pool.clone();
        let any_unblocked = self
            .entries
            .iter()
            .filter(|e| e.server.pool == pool)
            .any(|e| !e.blocked);

        for entry in &mut self.entries {
            if entry.server.pool == pool {
                entry.blocked = any_unblocked;
            }
        }
    }

    pub fn ping_progress(&self) -> Option<(usize, usize)> {
        self.ping_job
            .as_ref()
            .map(|job| (job.completed, job.total))
    }

    pub fn start_ping(&mut self) -> Result<()> {
        if self.pinging {
            return Ok(());
        }

        if self.blocking_active {
            self.apply_blocking()?;
        }

        let indices: Vec<usize> = if self.filter.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.visible_indices()
        };

        if indices.is_empty() {
            return Ok(());
        }

        let items: Vec<(usize, Server)> = indices
            .into_iter()
            .map(|idx| (idx, self.entries[idx].server.clone()))
            .collect();
        let total = items.len();
        let (tx, rx) = mpsc::unbounded_channel();

        self.rt.spawn(ping::ping_servers_progress(items, tx));
        self.ping_job = Some(PingJob {
            total,
            completed: 0,
            rx,
        });
        self.pinging = true;
        self.update_ping_status();
        Ok(())
    }

    pub fn poll_ping(&mut self) {
        let Some(job) = &mut self.ping_job else {
            return;
        };

        while let Ok((idx, result)) = job.rx.try_recv() {
            if let Some(entry) = self.entries.get_mut(idx) {
                entry.ping = Some(result);
            }
            job.completed += 1;
        }

        if job.completed >= job.total {
            self.ping_job = None;
            self.pinging = false;
            self.status_message = Some(StatusMessage::Info("пинг проверен".into()));
        } else {
            self.update_ping_status();
        }
    }

    fn update_ping_status(&mut self) {
        let Some((done, total)) = self.ping_progress() else {
            return;
        };
        let percent = done * 100 / total.max(1);
        self.status_message = Some(StatusMessage::Info(format!(
            "пинг {done}/{total} ({percent}%)"
        )));
    }

    pub fn apply_blocking(&mut self) -> Result<()> {
        let ips: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.blocked)
            .map(|e| e.server.ip.clone())
            .collect();

        firewall::apply(self.backend, &ips)?;
        self.settings.blocked = self
            .entries
            .iter()
            .filter(|e| e.blocked)
            .map(|e| e.server.name.clone())
            .collect();
        self.settings.save()?;
        self.blocking_active = !ips.is_empty();
        self.status_message = Some(StatusMessage::Info(format!(
            "заблокировано серверов: {}",
            ips.len()
        )));
        Ok(())
    }

    pub fn clear_blocking(&mut self) -> Result<()> {
        firewall::clear(self.backend)?;
        self.blocking_active = false;
        self.status_message = Some(StatusMessage::Info("блокировка снята".into()));
        Ok(())
    }
}

impl SettingsRow {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Backend),
            1 => Some(Self::Boot),
            2 => Some(Self::Systemd),
            3 => Some(Self::SyncOnStartup),
            4 => Some(Self::ApiLogin),
            _ => None,
        }
    }
}

pub fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let app = match load_app_with_spinner(&mut terminal) {
        Ok(app) => app,
        Err(err) => {
            restore_terminal(&mut terminal)?;
            return Err(err);
        }
    };

    let mut app = app;
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = std::time::Instant::now();

    let result = loop {
        app.poll_ping();
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match ui::handle_key(&mut app, key) {
                        Ok(true) => break Ok(()),
                        Ok(false) => {}
                        Err(err) => {
                            app.status_message =
                                Some(StatusMessage::Error(err.to_string()));
                        }
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = std::time::Instant::now();
        }
    };

    restore_terminal(&mut terminal)?;

    result
}

fn load_app_with_spinner(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<App> {
    let (tx, rx) = std_mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(App::new());
    });

    let mut tick = 0usize;
    loop {
        terminal.draw(|frame| ui::draw_loading(frame, "Загрузка серверов...", tick))?;
        tick = tick.wrapping_add(1);

        match rx.try_recv() {
            Ok(result) => return result,
            Err(std_mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(80)),
            Err(std_mpsc::TryRecvError::Disconnected) => {
                anyhow::bail!("инициализация прервана");
            }
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
