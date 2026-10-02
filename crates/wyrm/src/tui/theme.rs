use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub(crate) fn status_color(s: &str) -> Color {
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

pub(crate) fn short_status(s: &str) -> String {
    if let Some((head, _)) = s.split_once(' ') {
        head.to_string()
    } else {
        s.to_string()
    }
}

pub(crate) fn log_line_styled(line: &str) -> Line<'static> {
    log_line_styled_query(line, "")
}

pub(crate) fn log_line_styled_query(line: &str, query: &str) -> Line<'static> {
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
