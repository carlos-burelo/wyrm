use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Gauge, List, ListItem, Paragraph, Row, Sparkline, Table, Wrap,
    },
};

use super::data::*;
use super::state::*;
use super::theme::*;
use super::views_chrome::draw_empty;

pub(crate) fn draw_dashboard(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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

pub(crate) fn draw_list(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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

pub(crate) fn draw_detail(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
    let Some(app) = st.selected_app() else {
        f.render_widget(
            Paragraph::new("Sin selección")
                .block(Block::default().title(" detalle ").borders(Borders::ALL)),
            area,
        );
        return;
    };
    let (cpu, mem) = proc_metrics(&st.sys, app.pid);
    let log_path = crate::store::db::Database::log_path_for(&app.name);
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

pub(crate) fn draw_metrics(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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

pub(crate) fn draw_preview(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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
