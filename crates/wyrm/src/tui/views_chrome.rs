use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
};

use super::state::*;
use super::views_dashboard::draw_dashboard;
use super::views_logs::draw_logs;

pub(crate) fn draw(f: &mut ratatui::Frame, st: &TuiState) {
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

pub(crate) fn draw_header(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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

pub(crate) fn draw_tabs(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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

pub(crate) fn draw_empty(f: &mut ratatui::Frame, area: Rect) {
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

pub(crate) fn draw_help(f: &mut ratatui::Frame, area: Rect) {
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

pub(crate) fn draw_footer(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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
