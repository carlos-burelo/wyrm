use crate::daemon::AppStatus;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::data::{proc_cpu, proc_mem_mb};

#[derive(PartialEq, Clone, Copy)]
pub(crate) enum Tab {
    Dashboard,
    Logs,
    Help,
}

#[derive(PartialEq, Clone, Copy)]
pub(crate) enum SortMode {
    Name,
    Cpu,
    Mem,
    Uptime,
    Restarts,
}

impl SortMode {
    pub(crate) fn next(self) -> SortMode {
        match self {
            SortMode::Name => SortMode::Cpu,
            SortMode::Cpu => SortMode::Mem,
            SortMode::Mem => SortMode::Uptime,
            SortMode::Uptime => SortMode::Restarts,
            SortMode::Restarts => SortMode::Name,
        }
    }
    pub(crate) fn label(self) -> &'static str {
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
    pub(crate) fn all() -> &'static [Tab] {
        &[Tab::Dashboard, Tab::Logs, Tab::Help]
    }
    pub(crate) fn title(&self) -> &'static str {
        match self {
            Tab::Dashboard => " 1:Dashboard ",
            Tab::Logs => " 2:Logs ",
            Tab::Help => " 3:Help ",
        }
    }
    pub(crate) fn next(self) -> Tab {
        match self {
            Tab::Dashboard => Tab::Logs,
            Tab::Logs => Tab::Help,
            Tab::Help => Tab::Dashboard,
        }
    }
}

pub(crate) struct TuiState {
    pub(crate) apps: Vec<AppStatus>,
    pub(crate) selected: usize,
    pub(crate) tab: Tab,
    pub(crate) sort: SortMode,
    pub(crate) log_name: String,
    pub(crate) log_lines: Vec<String>,
    pub(crate) log_scroll: usize,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) error: Option<String>,
    pub(crate) daemon_on: bool,
    pub(crate) filter: String,
    pub(crate) filtering: bool,
    pub(crate) confirm_delete: bool,
    pub(crate) confirm_flush: bool,
    pub(crate) log_follow: bool,
    pub(crate) log_query: String,
    pub(crate) log_searching: bool,
    pub(crate) last_refresh: Instant,
    pub(crate) sys: sysinfo::System,
    pub(crate) cpu: f32,
    pub(crate) mem_used_gb: f64,
    pub(crate) mem_total_gb: f64,
    pub(crate) cpu_hist: VecDeque<u64>,
    pub(crate) mem_hist: VecDeque<u64>,
    pub(crate) app_cpu_hist: HashMap<String, VecDeque<u64>>,
    pub(crate) app_mem_hist: HashMap<String, VecDeque<u64>>,
}

impl TuiState {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn filtered(&self) -> Vec<(usize, &AppStatus)> {
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

    pub(crate) fn selected_app(&self) -> Option<AppStatus> {
        let list = self.filtered();
        if list.is_empty() {
            return None;
        }
        list.get(self.selected.min(list.len() - 1))
            .map(|(_, a)| (*a).clone())
    }
}
