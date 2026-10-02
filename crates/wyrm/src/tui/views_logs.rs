use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, List, ListItem},
};

use super::state::*;
use super::theme::*;

pub(crate) fn draw_logs(f: &mut ratatui::Frame, area: Rect, st: &TuiState) {
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
