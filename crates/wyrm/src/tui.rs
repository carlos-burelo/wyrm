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
    widgets::{
        Block, Borders, Cell, Gauge, List, ListItem, Paragraph, Row, Sparkline, Table, Tabs, Wrap,
    },
    Terminal,
};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Dashboard,
    Logs,
    Help,
}

#[derive(PartialEq, Clone, Copy)]
enum SortMode {
    Name,
    Cpu,
    Mem,
    Uptime,
    Restarts,
}

impl SortMode {
    fn next(self) -> SortMode {
        match self {
            SortMode::Name => SortMode::Cpu,
            SortMode::Cpu => SortMode::Mem,
            SortMode::Mem => SortMode::Uptime,
            SortMode::Uptime => SortMode::Restarts,
            SortMode::Restarts => SortMode::Name,
        }
    }
    fn label(self) -> &'static str {
        match self {
            SortMode::Name => "nombre",
            SortMode::Cpu => "cpu",
            SortMode::Mem => "mem",
            SortMode::Uptime => "uptime",
            SortMode::Restarts => "restarts",
        }
    }
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[Tab::Dashboard, Tab::Logs, Tab::Help]
    }
    fn title(&self) -> &'static str {
        match self {
            Tab::Dashboard => " 1:Dashboard ",
            Tab::Logs => " 2:Logs ",
            Tab::Help => " 3:Help ",
        }
    }
    fn next(self) -> Tab {
        match self {
            Tab::Dashboard => Tab::Logs,
            Tab::Logs => Tab::Help,
            Tab::Help => Tab::Dashboard,
        }
    }
}

struct TuiState {
    apps: Vec<AppStatus>,
    selected: usize,
    tab: Tab,
    sort: SortMode,
    log_name: String,
    log_lines: Vec<String>,
    log_scroll: usize,
    preview_lines: Vec<String>,
    error: Option<String>,
    daemon_on: bool,
    filter: String,
    filtering: bool,
    confirm_delete: bool,
    confirm_flush: bool,
    log_follow: bool,
    log_query: String,
    log_searching: bool,
    last_refresh: Instant,
    sys: sysinfo::System,
    cpu: f32,
    mem_used_gb: f64,
    mem_total_gb: f64,
    cpu_hist: VecDeque<u64>,
    mem_hist: VecDeque<u64>,
    app_cpu_hist: HashMap<String, VecDeque<u64>>,
    app_mem_hist: HashMap<String, VecDeque<u64>>,
}

impl TuiState {
    fn new() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        Self {
            apps: vec![],
            selected: 0,
            tab: Tab::Dashboard,
            sort: SortMode::Name,
            log_name: String::new(),
            log_lines: vec![],
            log_scroll: 0,
            preview_lines: vec![],
            error: None,
            daemon_on: false,
            filter: String::new(),
            filtering: false,
            confirm_delete: false,
            confirm_flush: false,
            log_follow: true,
            log_query: String::new(),
            log_searching: false,
            last_refresh: Instant::now() - Duration::from_secs(10),
            sys,
            cpu: 0.0,
            mem_used_gb: 0.0,
            mem_total_gb: 0.0,
            cpu_hist: VecDeque::with_capacity(61),
            mem_hist: VecDeque::with_capacity(61),
            app_cpu_hist: HashMap::new(),
            app_mem_hist: HashMap::new(),
        }
    }

    fn filtered(&self) -> Vec<(usize, &AppStatus)> {
        let f = self.filter.to_lowercase();
        let mut v: Vec<(usize, &AppStatus)> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| f.is_empty() || a.name.to_lowercase().contains(&f))
            .collect();
        // Orden superior a pm2 list: cuello ordenable en vivo.
        match self.sort {
            SortMode::Name => v.sort_by(|a, b| a.1.name.cmp(&b.1.name)),
            SortMode::Cpu => v.sort_by(|a, b| {
                proc_cpu(&self.sys, b.1.pid)
                    .partial_cmp(&proc_cpu(&self.sys, a.1.pid))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortMode::Mem => v.sort_by(|a, b| {
                proc_mem_mb(&self.sys, b.1.pid)
                    .partial_cmp(&proc_mem_mb(&self.sys, a.1.pid))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortMode::Uptime => v.sort_by(|a, b| b.1.uptime_secs.cmp(&a.1.uptime_secs)),
            SortMode::Restarts => v.sort_by(|a, b| b.1.restarts.cmp(&a.1.restarts)),
        }
        v
    }

    fn selected_app(&self) -> Option<AppStatus> {
        let list = self.filtered();
        if list.is_empty() {
            return None;
        }
        list.get(self.selected.min(list.len() - 1))
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
    // Historial global (60 puntos).
    push_hist(&mut st.cpu_hist, st.cpu.max(0.0) as u64);
    push_hist(&mut st.mem_hist, (st.mem_used_gb.max(0.0) * 1024.0) as u64);
    // Historial por app (solo las visibles para no crecer sin cota).
    for app in st.apps.clone() {
        let (cpu_f, mem_mb) = app_live_metrics(&st.sys, app.pid);
        push_hist(
            st.app_cpu_hist.entry(app.name.clone()).or_default(),
            cpu_f as u64,
        );
        push_hist(
            st.app_mem_hist.entry(app.name.clone()).or_default(),
            mem_mb as u64,
        );
    }
    // Preview del seleccionado para el panel derecho.
    if let Some(app) = st.selected_app() {
        let path = crate::db::Database::log_path_for(&app.name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let n = lines.len().saturating_sub(12);
            st.preview_lines = lines[n..].to_vec();
        } else {
            st.preview_lines = vec!["(sin logs todavía)".into()];
        }
    } else {
        st.preview_lines.clear();
    }
}

fn push_hist(hist: &mut VecDeque<u64>, v: u64) {
    if hist.len() >= 60 {
        hist.pop_front();
    }
    hist.push_back(v);
}

fn app_live_metrics(sys: &sysinfo::System, pid: Option<u32>) -> (f32, f64) {
    let Some(pid) = pid else {
        return (0.0, 0.0);
    };
    match sys.process(sysinfo::Pid::from_u32(pid)) {
        Some(p) => (p.cpu_usage(), p.memory() as f64 / 1_048_576.0),
        None => (0.0, 0.0),
    }
}

fn load_logs(st: &mut TuiState) {
    let path = crate::db::Database::log_path_for(&st.log_name);
    st.log_lines = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| "(sin logs todavía)".into())
        .lines()
        .map(|s| s.to_string())
        .collect();
    if st.log_follow {
        st.log_scroll = 0;
    }
}

fn flush_logs(st: &mut TuiState) {
    let path = crate::db::Database::log_path_for(&st.log_name);
    let _ = std::fs::write(&path, "");
    st.log_lines.clear();
    st.log_scroll = 0;
    st.error = Some(format!("Logs de {} vaciados", st.log_name));
}

fn open_logs_for_selected(st: &mut TuiState) {
    if let Some(app) = st.selected_app() {
        st.log_name = app.name.clone();
        st.log_follow = true;
        st.log_scroll = 0;
        load_logs(st);
        st.tab = Tab::Logs;
    }
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    st: &mut TuiState,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| draw(f, st))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
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

                if st.log_searching {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => st.log_searching = false,
                        KeyCode::Backspace => {
                            st.log_query.pop();
                        }
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            st.log_query.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }
                if st.confirm_flush {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            st.confirm_flush = false;
                            flush_logs(st);
                        }
                        _ => st.confirm_flush = false,
                    }
                    continue;
                }

                // Tabs globales.
                match key.code {
                    KeyCode::Char('1') => {
                        st.tab = Tab::Dashboard;
                        continue;
                    }
                    KeyCode::Char('2') => {
                        open_logs_for_selected(st);
                        continue;
                    }
                    KeyCode::Char('3') => {
                        st.tab = Tab::Help;
                        continue;
                    }
                    KeyCode::Tab => {
                        st.tab = st.tab.next();
                        if st.tab == Tab::Logs && st.log_name.is_empty() {
                            open_logs_for_selected(st);
                        }
                        continue;
                    }
                    _ => {}
                }

                match st.tab {
                    Tab::Dashboard => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('?') | KeyCode::Char('h') => st.tab = Tab::Help,
                        KeyCode::Char('/') => st.filtering = true,
                        KeyCode::Down | KeyCode::Char('j') => {
                            let n = st.filtered().len();
                            if n > 0 {
                                st.selected = (st.selected + 1) % n;
                                refresh_preview(st);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let n = st.filtered().len();
                            if n > 0 {
                                st.selected = st.selected.checked_sub(1).unwrap_or(n - 1);
                                refresh_preview(st);
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('l') => open_logs_for_selected(st),
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
                        KeyCode::Char('S') => {
                            if let Some(app) = st.selected_app() {
                                st.error = Some(format!("Arrancando {}…", app.name));
                                do_start(&app.name).await;
                                refresh_apps(st).await;
                                st.error = None;
                            }
                        }
                        KeyCode::Char('o') => {
                            st.sort = st.sort.next();
                            st.selected = 0;
                        }
                        KeyCode::Char('d') => {
                            if st.selected_app().is_some() {
                                st.confirm_delete = true;
                            }
                        }
                        _ => {}
                    },
                    Tab::Logs => match key.code {
                        KeyCode::Esc => {
                            if st.log_query.is_empty() {
                                st.tab = Tab::Dashboard;
                            } else {
                                st.log_query.clear();
                            }
                        }
                        KeyCode::Char('q') => st.tab = Tab::Dashboard,
                        KeyCode::Down | KeyCode::Char('j') => {
                            st.log_follow = false;
                            st.log_scroll = st.log_scroll.saturating_sub(1);
                            if st.log_scroll == 0 {
                                load_logs(st);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            st.log_follow = false;
                            load_logs(st);
                            st.log_scroll =
                                (st.log_scroll + 1).min(st.log_lines.len().saturating_sub(1));
                        }
                        KeyCode::Char('f') => {
                            st.log_follow = !st.log_follow;
                            if st.log_follow {
                                load_logs(st);
                            }
                        }
                        KeyCode::Char('/') => st.log_searching = true,
                        KeyCode::Char('F') => st.confirm_flush = true,
                        KeyCode::Char('r') => {
                            load_logs(st);
                            if st.log_follow {
                                st.log_scroll = 0;
                            }
                        }
                        KeyCode::Char('G') => {
                            st.log_follow = true;
                            load_logs(st);
                        }
                        _ => {}
                    },
                    Tab::Help => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                            st.tab = Tab::Dashboard
                        }
                        _ => {}
                    },
                }
            }
        }

        if st.last_refresh.elapsed() > Duration::from_secs(2) {
            let keep_selected = st.selected_app().map(|a| a.name);
            refresh_apps(st).await;
            // Re-selecciona por nombre para que el refresh no salte si cambia el orden.
            if let Some(name) = keep_selected {
                if let Some(idx) = st.filtered().iter().position(|(_, a)| a.name == name) {
                    st.selected = idx;
                }
            }
            if st.tab == Tab::Logs && st.log_follow && !st.log_name.is_empty() {
                load_logs(st);
            }
        }
    }
}

fn refresh_preview(st: &mut TuiState) {
    if let Some(app) = st.selected_app() {
        let path = crate::db::Database::log_path_for(&app.name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let n = lines.len().saturating_sub(12);
            st.preview_lines = lines[n..].to_vec();
        } else {
            st.preview_lines = vec!["(sin logs todavía)".into()];
        }
    }
}

async fn do_action(action: &str, name: &str) {
    let _ = crate::ipc::send_request(action, serde_json::json!({ "name": name })).await;
}

async fn do_start(name: &str) {
    // Revive STOPPED desde DB: carga AppConfig y envía START al demonio.
    // Ventaja sobre pm2: no necesitas cwd ni ecosystem a mano.
    let owned = name.to_string();
    let cfg = tokio::task::spawn_blocking(move || {
        crate::db::Database::init()
            .and_then(|db| db.get_app(&owned))
            .unwrap_or(None)
            .map(|r| r.to_app_config())
    })
    .await
    .unwrap_or(None);
    if let Some(cfg) = cfg {
        if let Ok(payload) = serde_json::to_value(&cfg) {
            let _ = crate::ipc::send_request("START", payload).await;
        }
    }
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
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, chunks[0], st);
    draw_tabs(f, chunks[1], st);
    match st.tab {
        Tab::Dashboard => draw_dashboard(f, chunks[2], st),
        Tab::Logs => draw_logs(f, chunks[2], st),
        Tab::Help => draw_help(f, chunks[2]),
    }
    draw_footer(f, chunks[3], st);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let daemon = if st.daemon_on {
        Span::styled("● daemon on", Style::default().fg(Color::Green))
    } else {
        Span::styled("● daemon off", Style::default().fg(Color::Red))
    };
    let running = st
        .apps
        .iter()
        .filter(|a| a.status.starts_with("RUNNING"))
        .count();
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
            "   CPU {:.1}%   MEM {:.1}/{:.1} GB   {}/{} running",
            st.cpu,
            st.mem_used_gb,
            st.mem_total_gb,
            running,
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

fn draw_tabs(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();
    let idx = Tab::all().iter().position(|t| *t == st.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(idx)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn draw_dashboard(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    if st.apps.is_empty() {
        draw_empty(f, area);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);
    draw_list(f, cols[0], st);

    // Derecha: detalle, métricas con sparklines, preview logs.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(38),
            Constraint::Percentage(32),
            Constraint::Percentage(30),
        ])
        .split(cols[1]);
    draw_detail(f, right[0], st);
    draw_metrics(f, right[1], st);
    draw_preview(f, right[2], st);
}

fn draw_empty(f: &mut ratatui::Frame, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Sin aplicaciones en supervisión",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  wyrm start        # en tu proyecto Node (auto-detecta)"),
        Line::from("  wyrm daemon       # arranca el demonio en dev"),
        Line::from("  wyrm service install   # producción Windows Server"),
        Line::from(""),
        Line::from(Span::styled(
            "Pulsa q para salir · ? ayuda",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(
        Paragraph::new(text)
            .block(Block::default().title(" wyrm ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_list(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let rows_data = st.filtered();
    let header = Row::new(vec!["NAME", "STATUS", "PID", "CPU%", "MEM"]).style(
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
                    short_status(&a.status),
                    Style::default().fg(status_color(&a.status)),
                )),
                Cell::from(a.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())),
                Cell::from(cpu),
                Cell::from(mem),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Percentage(30),
        Constraint::Percentage(28),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(9),
    ];
    let t = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(format!(
                    " aplicaciones ({}) [o:ord:{}] ",
                    rows_data.len(),
                    st.sort.label()
                ))
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(t, area);
}

fn short_status(s: &str) -> String {
    if let Some((head, _)) = s.split_once(' ') {
        head.to_string()
    } else {
        s.to_string()
    }
}

fn draw_detail(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let Some(app) = st.selected_app() else {
        f.render_widget(
            Paragraph::new("Sin selección")
                .block(Block::default().title(" detalle ").borders(Borders::ALL)),
            area,
        );
        return;
    };
    let (cpu, mem) = proc_metrics(&st.sys, app.pid);
    let log_path = crate::db::Database::log_path_for(&app.name);
    let mem_pct = if st.mem_total_gb > 0.0 {
        (st.mem_used_gb / st.mem_total_gb * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let sys_line = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(area);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                app.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                app.status.clone(),
                Style::default().fg(status_color(&app.status)),
            ),
        ]),
        Line::from(format!(
            "pid {}   cpu {}%   mem {}   restarts {}   uptime {}",
            app.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            cpu,
            mem,
            app.restarts,
            fmt_uptime(app.uptime_secs)
        )),
        Line::from(format!("exec {}", app.executable)),
        Line::from(format!("cwd  {}", app.cwd)),
        Line::from(format!("log  {}", log_path.display())),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" detalle ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        sys_line[0],
    );
    f.render_widget(
        Gauge::default()
            .block(Block::default())
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(mem_pct as f64 / 100.0)
            .label(format!(
                "SYS MEM {:.1}/{:.1} GB",
                st.mem_used_gb, st.mem_total_gb
            )),
        sys_line[1],
    );
}

fn draw_metrics(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let selected = st.selected_app().map(|a| a.name).unwrap_or_default();

    let cpu_data: Vec<u64> = st
        .app_cpu_hist
        .get(&selected)
        .map(|h| h.iter().copied().collect())
        .unwrap_or_default();
    let mem_data: Vec<u64> = st
        .app_mem_hist
        .get(&selected)
        .map(|h| h.iter().copied().collect())
        .unwrap_or_default();

    let cpu_max = cpu_data.iter().copied().max().unwrap_or(100).max(10);
    let mem_max = mem_data.iter().copied().max().unwrap_or(100).max(10);

    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(format!(" cpu%:{selected} (max {cpu_max}) "))
                    .borders(Borders::ALL),
            )
            .data(&cpu_data)
            .max(cpu_max)
            .style(Style::default().fg(Color::Cyan)),
        cols[0],
    );
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(format!(" mem MB:{selected} (max {mem_max}) "))
                    .borders(Borders::ALL),
            )
            .data(&mem_data)
            .max(mem_max)
            .style(Style::default().fg(Color::Magenta)),
        cols[1],
    );
}

fn draw_preview(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let items: Vec<ListItem> = st
        .preview_lines
        .iter()
        .map(|l| ListItem::new(log_line_styled(l)))
        .collect();
    let title = match st.selected_app() {
        Some(a) => format!(" preview:{} (enter=logs) ", a.name),
        None => " preview ".into(),
    };
    f.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn log_line_styled(line: &str) -> Line<'static> {
    log_line_styled_query(line, "")
}

fn log_line_styled_query(line: &str, query: &str) -> Line<'static> {
    let lower = line.to_lowercase();
    let base = if lower.contains("error")
        || lower.contains("fail")
        || lower.contains("crash")
        || lower.contains("panic")
        || lower.contains("exception")
    {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if lower.contains("warn") {
        Style::default().fg(Color::Yellow)
    } else if lower.contains("ok")
        || lower.contains("success")
        || lower.contains("listening")
        || lower.contains("ready")
        || lower.contains("start")
    {
        Style::default().fg(Color::Green)
    } else if lower.contains("debug") || lower.contains("trace") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Gray)
    };
    if query.is_empty() {
        return Line::from(Span::styled(line.to_string(), base));
    }
    // Resalta ocurrencias del query con fondo amarillo.
    let ql = query.to_lowercase();
    if let Some(pos) = lower.find(&ql) {
        let end = (pos + query.len()).min(line.len());
        // Ojo: índices byte, válido para ASCII (queries típicos). Fallback simple.
        if line.is_ascii() {
            return Line::from(vec![
                Span::styled(line[..pos].to_string(), base),
                Span::styled(
                    line[pos..end].to_string(),
                    base.bg(Color::Yellow).fg(Color::Black),
                ),
                Span::styled(line[end..].to_string(), base),
            ]);
        }
    }
    Line::from(Span::styled(line.to_string(), base))
}

fn proc_metrics(sys: &sysinfo::System, pid: Option<u32>) -> (String, String) {
    let (cpu, mem) = app_live_metrics(sys, pid);
    if pid.is_none() {
        return ("-".into(), "-".into());
    }
    let id = sysinfo::Pid::from_u32(pid.unwrap_or(0));
    if sys.process(id).is_none() {
        return ("-".into(), "-".into());
    }
    (format!("{cpu:.1}"), format!("{mem:.0}M"))
}

fn proc_cpu(sys: &sysinfo::System, pid: Option<u32>) -> f32 {
    app_live_metrics(sys, pid).0
}

fn proc_mem_mb(sys: &sysinfo::System, pid: Option<u32>) -> f64 {
    app_live_metrics(sys, pid).1
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
    let q = st.log_query.to_lowercase();
    let filtered: Vec<&String> = if q.is_empty() {
        st.log_lines.iter().collect()
    } else {
        st.log_lines
            .iter()
            .filter(|l| l.to_lowercase().contains(&q))
            .collect()
    };
    let total = filtered.len();
    let height = area.height.saturating_sub(2) as usize;
    let end = total.saturating_sub(st.log_scroll);
    let start = end.saturating_sub(height.max(1));
    let visible: Vec<ListItem> = filtered[start..end]
        .iter()
        .map(|l| ListItem::new(log_line_styled_query(l, &st.log_query)))
        .collect();
    let follow = if st.log_follow {
        "●follow"
    } else {
        "○follow(f)"
    };
    let title =
        if st.log_searching {
            format!(" buscar: {}▊  (enter/esc sale) ", st.log_query)
        } else if st.confirm_flush {
            format!("¿Vaciar logs de {}? y=sí otra=no", st.log_name)
        } else if !st.log_query.is_empty() {
            format!(
                " logs:{} [{}/{} filtradas, {}] (/ buscar, esc limpiar) ",
                st.log_name,
                total,
                st.log_lines.len(),
                follow
            )
        } else {
            format!(
            " logs:{} ({} líneas, {}) j/k scroll · f follow · / buscar · F vaciar · esc volver ",
            st.log_name, st.log_lines.len(), follow
        )
        };
    let list = List::new(visible).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(list, area);
}

fn draw_help(f: &mut ratatui::Frame, area: Rect) {
    let text = vec![
        Line::from(Span::styled(
            "Wyrm TUI — mejor que pm2 monit",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  1/2/3 o tab    Dashboard / Logs / Help"),
        Line::from("  j/k o ↑/↓      navegar apps o scroll logs"),
        Line::from("  enter / l      ver logs de la app"),
        Line::from("  r              restart"),
        Line::from("  s / S          stop / start (revive STOPPED desde DB)"),
        Line::from("  d luego y      delete con confirmación"),
        Line::from("  o              ciclo orden: nombre → cpu → mem → uptime → restarts"),
        Line::from("  /              filtrar apps (dashboard) o buscar en logs"),
        Line::from("  f              follow on/off en logs"),
        Line::from("  F luego y      vaciar log actual"),
        Line::from("  G              ir al final (follow)"),
        Line::from("  q / esc        salir o volver"),
        Line::from(""),
        Line::from("Arranque: `wyrm daemon` en dev o `wyrm service install` en Server."),
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
    } else if st.confirm_flush {
        format!("¿Vaciar logs de {}?  y=sí  otra tecla=no", st.log_name)
    } else if let Some(e) = &st.error {
        format!("! {e}")
    } else if st.filtering {
        format!("filtro: {} ▊  (enter/esc sale)", st.filter)
    } else if st.log_searching {
        format!("buscar en logs: {}▊  (enter/esc sale)", st.log_query)
    } else {
        match st.tab {
            Tab::Dashboard => {
                " 1/2/3 tabs · j/k mover · enter logs · r restart · s stop · S start · d delete · o orden · / filtrar · q salir "
                    .into()
            }
            Tab::Logs => {
                " j/k scroll · f follow · / buscar · F vaciar · G final · r recargar · esc volver ".into()
            }
            Tab::Help => " esc volver ".into(),
        }
    };
    let style = if st.confirm_delete || st.confirm_flush || st.error.is_some() {
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
