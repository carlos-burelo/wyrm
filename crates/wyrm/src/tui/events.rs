use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use super::actions::*;
use super::data::*;
use super::state::*;
use super::views_chrome::draw;

pub(crate) async fn event_loop(
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
