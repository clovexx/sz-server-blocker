use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::{App, InputMode, Screen, SettingsRow, StatusMessage};
use crate::ping::format_latency;
use crate::systemd;

pub fn draw_loading(frame: &mut Frame, message: &str, tick: usize) {
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][tick % 10];
    let area = centered_rect(48, 3, frame.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Блокировщик серверов STALZONE ");
    let line = Line::from(format!("{spinner}  {message}"));
    frame.render_widget(
        Paragraph::new(line)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    Rect { x, y, width, height }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::Servers => draw_servers(frame, app),
        Screen::Settings => draw_settings_screen(frame, app),
    }
}

fn draw_servers(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);
    draw_table(frame, chunks[1], app);
    draw_servers_footer(frame, chunks[2], app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let filter_text = match app.input_mode {
        InputMode::Filter => format!("Фильтр: {}_", app.filter),
        _ => format!("Фильтр: {}", app.filter),
    };

    let active = if app.blocking_active {
        "блокировка вкл"
    } else {
        "блокировка выкл"
    };
    let status = status_span(app);

    let line = Line::from(vec![
        Span::raw(filter_text),
        Span::raw("   Бэкенд: "),
        Span::styled(
            app.backend.name(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(
            active,
            if app.blocking_active {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::raw("  "),
        status,
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Блокировщик серверов STALZONE ");

    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let mark_w = 4u16;
    let ip_w = 16u16;
    let pool_w = 12u16;
    let ping_w = 6u16;

    let header = Row::new(vec!["", "Имя", "IP", "Пул", "мс"])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let visible = app.visible_indices();
    let rows: Vec<Row> = visible
        .iter()
        .map(|&server_idx| {
            let entry = &app.entries[server_idx];
            let mark = if entry.blocked { "[x]" } else { "[ ]" };
            let ping = format_latency(entry.ping);

            Row::new(vec![
                Cell::from(mark),
                Cell::from(entry.server.name.clone()),
                Cell::from(entry.server.ip.clone()),
                Cell::from(entry.server.pool.clone()),
                Cell::from(ping),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(mark_w),
        Constraint::Fill(1),
        Constraint::Length(ip_w),
        Constraint::Length(pool_w),
        Constraint::Length(ping_w),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" [x] — заблокировать  |  пинг: мс, тайм, нет, — "),
        );

    app.table_state.select(Some(app.list_index));
    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_servers_footer(frame: &mut Frame, area: Rect, app: &App) {
    let help = if app.pinging {
        if let Some((done, total)) = app.ping_progress() {
            format!(
                "Пинг {done}/{total} {}  (можно листать список)",
                progress_bar(done, total, 20)
            )
        } else {
            "Пинг...".into()
        }
    } else {
        "Пробел:блок  a:пул  p:пинг  r:синк  b:применить  u:снять  /:поиск  s:настройки  q:выход".into()
    };

    let count = format!(
        " выбрано {}/{} ",
        app.entries.iter().filter(|e| e.blocked).count(),
        app.entries.len()
    );

    let line = Line::from(vec![Span::raw(help), Span::raw(count)]);
    let block = Block::default().borders(Borders::ALL);
    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    let total = total.max(1);
    let filled = done.saturating_mul(width) / total;
    format!(
        "[{}{}]",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn draw_settings_screen(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let status = status_span(app);
    let header = Line::from(vec![
        Span::raw("Настройки"),
        Span::raw("   "),
        status,
    ]);
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Блокировщик серверов STALZONE "),
        ),
        chunks[0],
    );

    let boot = if app.settings.apply_on_boot { "[x]" } else { "[ ]" };
    let sync_on_startup = if app.settings.sync_on_startup {
        "[x]"
    } else {
        "[ ]"
    };
    let api_login = &app.settings.api_login;
    let binary = systemd::binary_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".into());

    let rows = [
        (
            SettingsRow::Backend,
            format!("Бэкенд:  {}", app.settings.backend.label()),
            "←/→ переключить",
        ),
        (
            SettingsRow::Boot,
            format!("При загрузке:  {boot}"),
            "Пробел (root)",
        ),
        (
            SettingsRow::Systemd,
            format!("Systemd:  {}", app.systemd_status),
            "Enter установить",
        ),
        (
            SettingsRow::SyncOnStartup,
            format!("Автосинк:  {sync_on_startup}"),
            "Пробел",
        ),
        (
            SettingsRow::ApiLogin,
            format!("Логин API:  {api_login}"),
            "Enter изменить",
        ),
    ];

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|(_, label, hint)| Row::new(vec![Cell::from(label.as_str()), Cell::from(*hint)]))
        .collect();

    let table = Table::new(
        table_rows,
        [Constraint::Percentage(65), Constraint::Percentage(35)],
    )
    .row_highlight_style(Style::default().bg(Color::DarkGray))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Бинарник: {binary} ")),
    );

    app.settings_table_state.select(Some(app.settings_index));
    frame.render_stateful_widget(table, chunks[1], &mut app.settings_table_state);

    let footer = if app.input_mode == InputMode::ApiLogin {
        Line::from(vec![Span::raw(format!(
            "Логин: {}_   Enter:сохранить  Esc:отмена",
            app.api_login_draft
        ))])
    } else {
        Line::from(vec![Span::raw(
            "↑↓:строка  ←/→:бэкенд  Пробел:перекл.  Enter:действие  Esc:назад  q:выход",
        )])
    };
    frame.render_widget(
        Paragraph::new(footer).block(Block::default().borders(Borders::ALL)),
        chunks[2],
    );
}


fn status_span(app: &App) -> Span<'_> {
    match &app.status_message {
        Some(StatusMessage::Info(msg)) => {
            Span::styled(msg.clone(), Style::default().fg(Color::Yellow))
        }
        Some(StatusMessage::Error(msg)) => {
            Span::styled(format!("ОШИБКА: {msg}"), Style::default().fg(Color::Red))
        }
        None => Span::raw(String::new()),
    }
}

pub fn handle_key(app: &mut App, key: event::KeyEvent) -> Result<bool> {
    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }

    match app.screen {
        Screen::Servers => handle_servers_key(app, key),
        Screen::Settings => handle_settings_key(app, key),
    }
}

fn handle_servers_key(app: &mut App, key: event::KeyEvent) -> Result<bool> {
    if app.input_mode == InputMode::Filter {
        return Ok(handle_filter_input(app, key));
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('й') => return Ok(true),
        KeyCode::Char('s') | KeyCode::Char('ы') if !app.pinging => app.open_settings(),
        KeyCode::Char('p') | KeyCode::Char('з') if !app.pinging => app.start_ping()?,
        KeyCode::Char('b') | KeyCode::Char('и') if !app.pinging => app.apply_blocking()?,
        KeyCode::Char('u') | KeyCode::Char('г') if !app.pinging => app.clear_blocking()?,
        KeyCode::Char('r') | KeyCode::Char('к') if !app.pinging => app.sync_servers()?,
        KeyCode::Char('a') | KeyCode::Char('ф') if !app.pinging => app.toggle_pool(),
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Filter;
            app.filter.clear();
        }
        KeyCode::Char(' ') if !app.pinging => app.toggle_selected(),
        KeyCode::Up => app.move_selection(-1),
        KeyCode::Down => app.move_selection(1),
        KeyCode::PageUp => app.move_selection(-10),
        KeyCode::PageDown => app.move_selection(10),
        KeyCode::Home => {
            app.list_index = 0;
            app.reset_list_scroll();
        }
        KeyCode::End => {
            let visible = app.visible_indices();
            app.list_index = visible.len().saturating_sub(1);
            app.table_state.select(Some(app.list_index));
        }
        _ => {}
    }

    Ok(false)
}

fn handle_settings_key(app: &mut App, key: event::KeyEvent) -> Result<bool> {
    if app.input_mode == InputMode::ApiLogin {
        return Ok(handle_api_login_input(app, key));
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('й') => return Ok(true),
        KeyCode::Esc => app.close_settings(),
        KeyCode::Up => app.move_settings_selection(-1),
        KeyCode::Down => app.move_settings_selection(1),
        KeyCode::Left => {
            if app.settings_index == SettingsRow::Backend as usize {
                app.cycle_backend(false)?;
            }
        }
        KeyCode::Right => {
            if app.settings_index == SettingsRow::Backend as usize {
                app.cycle_backend(true)?;
            }
        }
        KeyCode::Char(' ') => app.activate_settings_row()?,
        KeyCode::Enter => app.activate_settings_row()?,
        _ => {}
    }

    Ok(false)
}

fn handle_api_login_input(app: &mut App, key: event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => app.cancel_api_login_edit(),
        KeyCode::Enter => {
            if let Err(err) = app.save_api_login() {
                app.status_message = Some(StatusMessage::Error(err.to_string()));
            }
        }
        KeyCode::Backspace => {
            app.api_login_draft.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.api_login_draft.push(c);
        }
        _ => {}
    }

    false
}

fn handle_filter_input(app: &mut App, key: event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.list_index = 0;
            app.reset_list_scroll();
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            app.list_index = 0;
            app.reset_list_scroll();
        }
        KeyCode::Backspace => {
            app.filter.pop();
            app.list_index = 0;
            app.reset_list_scroll();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.filter.push(c);
            app.list_index = 0;
            app.reset_list_scroll();
        }
        _ => {}
    }

    false
}
