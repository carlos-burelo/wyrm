use crate::daemon::AppStatus;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap},
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone, Copy)]
enum View {
    List,
    Logs,
    Help,
}

struct TuiState {
    apps: Vec<AppStatus>,
    selected: usize,
    view: View,
    log_name: String,
    log_lines: Vec<String>,
    log_scroll: usize,
    error: Option<String>,
    daemon_on: bool,
    filter: String,
    filtering: bool,
    confirm_delete: bool,
    last_refresh: Instant,
    sys: sysinfo::System,
    cpu: f32,
    mem_used_gb: f64,
    mem_total_gb: f64,
}

impl TuiState {
    fn new() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        Self {
            apps: vec![],
            selected: 0,
            view: View::List,
            log_name: String::new(),
            log_lines: vec![],
            log_scroll: 0,
            error: None,
            daemon_on: false,
            filter: String::new(),
            filtering: false,
            confirm_delete: false,
            last_refresh: Instant::now() - Duration::from_secs(10),
            sys,
            cpu: 0.0,
            mem_used_gb: 0.0,
            mem_total_gb: 0.0,
        }
    }

    fn filtered(&self) -> Vec<(usize, &AppStatus)> {
        let f = self.filter.to_lowercase();
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| f.is_empty() || a.name.to_lowercase().contains(&f))
            .collect()
    }

    fn selected_app(&self) -> Option<AppStatus> {
        self.filtered()
            .get(self.selected.min(self.filtered().len().saturating_sub(1)))
            .map(|(_, a)| (*a).clone())
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut st = TuiState::new();
    refresh_apps(&mut st).await;

    let res = event_loop(&mut terminal, &mut st).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("TUI error: {e}");
    }
    Ok(())
}

async fn refresh_apps(st: &mut TuiState) {
    st.sys.refresh_all();
    st.cpu = st.sys.global_cpu_info().cpu_usage();
    st.mem_used_gb = st.sys.used_memory() as f64 / 1_073_741_824.0;
    st.mem_total_gb = st.sys.total_memory() as f64 / 1_073_741_824.0;

    match crate::ipc::send_request("LIST", serde_json::Value::Null).await {
        Ok(res) if res.is_ok() => {
            st.apps = serde_json::from_value(res.data.unwrap_or_default()).unwrap_or_default();
            st.daemon_on = true;
            st.error = None;
        }
        Ok(res) => {
            st.daemon_on = true;
            st.error = Some(res.message);
        }
        Err(_) => {
            st.daemon_on = false;
            // Fallback DB.
            let db_rows: Vec<crate::db::AppRecord> = tokio::task::spawn_blocking(|| {
                crate::db::Database::init()
                    .and_then(|db| db.list_apps())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            st.apps = db_rows
                .into_iter()
                .map(|r| AppStatus {
                    name: r.name,
                    status: "STOPPED (daemon off)".into(),
                    pid: None,
                    restarts: r.restarts as u32,
                    uptime_secs: 0,
                    executable: r.executable,
                    cwd: r.cwd,
                })
                .collect();
            st.apps.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
    st.last_refresh = Instant::now();
    if st.selected >= st.filtered().len() && !st.filtered().is_empty() {
        st.selected = st.filtered().len() - 1;
    }
}

fn load_logs(st: &mut TuiState) {
    let path = crate::db::Database::log_path_for(&st.log_name);
    st.log_lines = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| "(sin logs todavía)".into())
        .lines()
        .map(|s| s.to_string())
        .collect();
    // Quedamos abajo por defecto.
    st.log_scroll = 0;
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    st: &mut TuiState,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| draw(f, st))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // Modo filtro.
                if st.filtering {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => st.filtering = false,
                        KeyCode::Backspace => {
                            st.filter.pop();
                            st.selected = 0;
                        }
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            st.filter.push(c);
                            st.selected = 0;
                        }
                        _ => {}
                    }
                    continue;
                }
                // Confirm delete.
                if st.confirm_delete {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            st.confirm_delete = false;
                            if let Some(app) = st.selected_app() {
                                do_delete(&app.name).await;
                                refresh_apps(st).await;
                            }
                        }
                        _ => st.confirm_delete = false,
                    }
                    continue;
                }

                match st.view {
                    View::List => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('?') | KeyCode::Char('h') => st.view = View::Help,
                        KeyCode::Char('/') => st.filtering = true,
                        KeyCode::Down | KeyCode::Char('j') => {
                            let n = st.filtered().len();
                            if n > 0 {
                                st.selected = (st.selected + 1) % n;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let n = st.filtered().len();
                            if n > 0 {
                                st.selected = st.selected.checked_sub(1).unwrap_or(n - 1);
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('l') => {
                            if let Some(app) = st.selected_app() {
                                st.log_name = app.name.clone();
                                load_logs(st);
                                st.view = View::Logs;
                            }
                        }
                        KeyCode::Char('r') => {
                            if let Some(app) = st.selected_app() {
                                st.error = Some(format!("Reiniciando {}…", app.name));
                                do_action("RESTART", &app.name).await;
                                refresh_apps(st).await;
                                st.error = None;
                            }
                        }
                        KeyCode::Char('s') => {
                            if let Some(app) = st.selected_app() {
                                do_action("STOP", &app.name).await;
                                refresh_apps(st).await;
                            }
                        }
                        KeyCode::Char('d') => {
                            if st.selected_app().is_some() {
                                st.confirm_delete = true;
                            }
                        }
                        _ => {}
                    },
                    View::Logs => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => st.view = View::List,
                        KeyCode::Down | KeyCode::Char('j') => {
                            st.log_scroll = st.log_scroll.saturating_sub(1);
                            if st.log_scroll == 0 {
                                load_logs(st);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            load_logs(st);
                            st.log_scroll =
                                (st.log_scroll + 1).min(st.log_lines.len().saturating_sub(1));
                        }
                        KeyCode::Char('r') => {
                            load_logs(st);
                            st.log_scroll = 0;
                        }
                        _ => {}
                    },
                    View::Help => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                            st.view = View::List
                        }
                        _ => {}
                    },
                }
            }
        }

        if st.last_refresh.elapsed() > Duration::from_secs(2) {
            refresh_apps(st).await;
            if st.view == View::Logs && st.log_scroll == 0 {
                load_logs(st);
            }
        }
    }
}

async fn do_action(action: &str, name: &str) {
    let _ = crate::ipc::send_request(action, serde_json::json!({ "name": name })).await;
}

async fn do_delete(name: &str) {
    let _ = crate::ipc::send_request("DELETE", serde_json::json!({ "name": name })).await;
}

fn status_color(s: &str) -> Color {
    if s.starts_with("RUNNING") {
        Color::Green
    } else if s.starts_with("CRASHED") {
        Color::Red
    } else if s.contains("off") {
        Color::DarkGray
    } else {
        Color::Yellow
    }
}

fn draw(f: &mut ratatui::Frame, st: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, chunks[0], st);
    match st.view {
        View::List => draw_list(f, chunks[1], st),
        View::Logs => draw_logs(f, chunks[1], st),
        View::Help => draw_help(f, chunks[1]),
    }
    draw_footer(f, chunks[2], st);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let daemon = if st.daemon_on {
        Span::styled("● daemon on", Style::default().fg(Color::Green))
    } else {
        Span::styled("● daemon off", Style::default().fg(Color::Red))
    };
    let title = Line::from(vec![
        Span::styled(
            " wyrm ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        daemon,
        Span::raw(format!(
            "   CPU {:.1}%   MEM {:.1}/{:.1} GB   apps {}",
            st.cpu,
            st.mem_used_gb,
            st.mem_total_gb,
            st.apps.len()
        )),
        Span::raw(if st.filter.is_empty() {
            String::new()
        } else {
            format!("   /{}", st.filter)
        }),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(title).block(block), area);
}

fn draw_list(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let rows_data = st.filtered();
    let header = Row::new(vec![
        "NAME", "STATUS", "PID", "CPU%", "MEM", "RESTARTS", "UPTIME",
    ])
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let rows: Vec<Row> = rows_data
        .iter()
        .enumerate()
        .map(|(i, (_, a))| {
            let (cpu, mem) = proc_metrics(&st.sys, a.pid);
            let style = if i == st.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(a.name.clone()),
                Cell::from(Span::styled(
                    a.status.clone(),
                    Style::default().fg(status_color(&a.status)),
                )),
                Cell::from(a.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())),
                Cell::from(cpu),
                Cell::from(mem),
                Cell::from(a.restarts.to_string()),
                Cell::from(fmt_uptime(a.uptime_secs)),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Percentage(22),
        Constraint::Percentage(24),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    let t = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(format!(" aplicaciones ({}) ", rows_data.len()))
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(t, area);
}

fn proc_metrics(sys: &sysinfo::System, pid: Option<u32>) -> (String, String) {
    let Some(pid) = pid else {
        return ("-".into(), "-".into());
    };
    let id = sysinfo::Pid::from_u32(pid);
    match sys.process(id) {
        Some(p) => (
            format!("{:.1}", p.cpu_usage()),
            format!("{:.0}M", p.memory() as f64 / 1_048_576.0),
        ),
        None => ("-".into(), "-".into()),
    }
}

fn fmt_uptime(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn draw_logs(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let total = st.log_lines.len();
    let height = area.height.saturating_sub(2) as usize;
    let end = total.saturating_sub(st.log_scroll);
    let start = end.saturating_sub(height.max(1));
    let visible: Vec<ListItem> = st.log_lines[start..end]
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();
    let list = List::new(visible).block(
        Block::default()
            .title(format!(
                " logs:{} ({} líneas, j/k scroll, r recargar, esc volver) ",
                st.log_name, total
            ))
            .borders(Borders::ALL),
    );
    f.render_widget(list, area);
}

fn draw_help(f: &mut ratatui::Frame, area: Rect) {
    let text = vec![
        Line::from("Wyrm TUI — ayuda"),
        Line::from(""),
        Line::from("  j/k o ↑/↓   navegar"),
        Line::from("  enter / l    ver logs"),
        Line::from("  r            restart app seleccionada"),
        Line::from("  s            stop app seleccionada"),
        Line::from("  d luego y    delete app (confirma)"),
        Line::from("  /            filtrar por nombre (esc sale)"),
        Line::from("  ?            esta ayuda"),
        Line::from("  q / esc      salir o volver"),
        Line::from(""),
        Line::from("Arranque: `wyrm daemon` en otra terminal o `wyrm service install`."),
    ];
    f.render_widget(
        Paragraph::new(text)
            .block(Block::default().title(" ayuda ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let msg = if st.confirm_delete {
        if let Some(app) = st.selected_app() {
            format!("¿Eliminar {}?  y=sí  otra tecla=no", app.name)
        } else {
            "¿Eliminar?".into()
        }
    } else if let Some(e) = &st.error {
        format!("! {e}")
    } else if st.filtering {
        format!("filtro: {} ▊  (enter/esc sale)", st.filter)
    } else {
        match st.view {
            View::List => " j/k mover · enter logs · r restart · s stop · d delete · / filtrar · ? ayuda · q salir ".into(),
            View::Logs => " j/k scroll · r recargar · esc volver ".into(),
            View::Help => " esc volver ".into(),
        }
    };
    let style = if st.confirm_delete || st.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(msg, style)))
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}
