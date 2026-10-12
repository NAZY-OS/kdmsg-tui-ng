use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use regex::Regex;
use std::fs;
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};

const MAX_LOG_LINES: usize = 5000;

#[derive(PartialEq, Clone, Copy, Debug)]
enum LogFilter {
    All,
    Warn,
    Info,
    Err,
    Notice,
}

#[derive(PartialEq, Clone, Copy)]
enum LogSource {
    Dmesg,
    Journalctl,
}

enum Message {
    LogsUpdated(Vec<String>),
}

struct LogViewState {
    filtered_indices: Vec<usize>,
    last_len: usize,
    last_search: String,
    last_filter: LogFilter,
}

impl LogViewState {
    fn new() -> Self {
        Self {
            filtered_indices: Vec::new(),
            last_len: 0,
            last_search: String::new(),
            last_filter: LogFilter::All,
        }
    }

    fn update_if_needed(&mut self, lines: &[String], search: &str, filter: LogFilter) {
        if lines.len() == self.last_len && search == self.last_search && filter == self.last_filter {
            return;
        }

        self.last_len = lines.len();
        self.last_search = search.to_string();
        self.last_filter = filter;

        let search_pattern = if search.is_empty() {
            None
        } else {
            let escaped_parts: Vec<String> = search
                .split_whitespace()
                .map(|part| regex::escape(part))
                .collect();
            let combined = format!("(?i){}", escaped_parts.join(".*"));
            regex::Regex::new(&combined).ok()
        };

        self.filtered_indices = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                let matches_search = if let Some(ref reg) = search_pattern {
                    reg.is_match(l)
                } else {
                    true
                };

                let matches_filter = match filter {
                    LogFilter::All => true,
                    LogFilter::Warn => {
                        let low = l.to_lowercase();
                        low.contains("warn") || low.contains("warning") || low.contains("deprecat") || low.contains("taint") 
                        || low.contains("bad") || low.contains("degraded") || low.contains("limit") || low.contains("drop") 
                        || low.contains("slow") || low.contains("bug") || low.contains("<4>") || low.contains("restricted")
                    }
                    LogFilter::Info => {
                        let low = l.to_lowercase();
                        low.contains("info") || low.contains("enabling") || low.contains("registered") || low.contains("detected") 
                        || low.contains("libata") || low.contains("usb") || low.contains("pci") || low.contains("cpu") 
                        || low.contains("acpi") || low.contains("net") || low.contains("init") || low.contains("link") 
                        || low.contains("driver") || low.contains("scsi") || low.contains("<6>") || low.contains("<5>")
                    }
                    LogFilter::Err => {
                        let low = l.to_lowercase();
                        low.contains("err") || low.contains("fail") || low.contains("crit") || low.contains("error") 
                        || low.contains("fault") || low.contains("segfault") || low.contains("oops") || low.contains("denied")
                        || low.contains("timeout") || low.contains("dead") || low.contains("<3>") || low.contains("<2>") || low.contains("<1>") || low.contains("<0>")
                    }
                    LogFilter::Notice => {
                        let low = l.to_lowercase();
                        low.contains("notice") || low.contains("note") || low.contains("ready") || low.contains("started") 
                        || low.contains("version") || low.contains("boot") || low.contains("mount") || low.contains("system")
                        || low.contains("command line") || low.contains("memory:")
                    }
                };
                matches_search && matches_filter
            })
            .map(|(idx, _)| idx)
            .collect();
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut dmesg_lines: Vec<String> = Vec::new();
    let mut search = String::new();
    let mut scroll_y: usize = 0;
    let mut scroll_x: usize = 0;
    let mut port_scroll: usize = 0;
    let mut auto_scroll = true;
    let mut show_info_panel = true;
    let mut show_help_modal = false;
    let mut active_filter = LogFilter::All;
    let mut log_source = LogSource::Dmesg;
    let mut export_status: Option<String> = None;
    let mut export_timer: Option<Instant> = None;

    let (tx, rx): (std::sync::mpsc::Sender<Message>, std::sync::mpsc::Receiver<Message>) = std::sync::mpsc::channel();

    spawn_log_fetcher(tx.clone(), log_source);

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(5);

    let res = run_app(
        &mut terminal,
        &mut dmesg_lines,
        &mut search,
        &mut scroll_y,
        &mut scroll_x,
        &mut port_scroll,
        &mut auto_scroll,
        &mut show_info_panel,
        &mut show_help_modal,
        &mut active_filter,
        &mut log_source,
        &mut export_status,
        &mut export_timer,
        &tx,
        &rx,
        &mut sys,
        &mut nets,
        &mut last_refresh,
        refresh_interval,
    );

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

fn spawn_log_fetcher(tx: std::sync::mpsc::Sender<Message>, source: LogSource) {
    std::thread::spawn(move || {
        let mut lines = match source {
            LogSource::Dmesg => {
                if let Ok(out) = Command::new("dmesg").args(["-T"]).output() {
                    let raw = String::from_utf8_lossy(&out.stdout);
                    let year_regex = Regex::new(r" \d{4}\]").unwrap();
                    raw.lines()
                        .map(|s| year_regex.replace(s, "]").to_string())
                        .collect()
                } else {
                    Vec::new()
                }
            }
            LogSource::Journalctl => {
                if let Ok(out) = Command::new("journalctl").args(["-b", "-o", "short"]).output() {
                    let raw = String::from_utf8_lossy(&out.stdout);
                    raw.lines().map(|s| s.to_string()).collect()
                } else {
                    Vec::new()
                }
            }
        };

        // Bounded Buffer: Max. 5000 Zeilen behalten, um RAM & CPU zu schonen
        if lines.len() > MAX_LOG_LINES {
            let start = lines.len() - MAX_LOG_LINES;
            lines = lines.drain(start..).collect();
        }

        let _ = tx.send(Message::LogsUpdated(lines));
    });
}

fn render_bar(current: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return "[--------------------]".to_string();
    }
    let filled = ((current as f64 / total as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!("[{}{}] {:.1}%", "█".repeat(filled), "-".repeat(empty), (current as f64 / total as f64) * 100.0)
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else {
        format!("{}m {}s", mins, secs)
    }
}

fn get_exact_boot_title() -> String {
    if let Ok(content) = std::fs::read_to_string("/proc/stat") {
        for line in content.lines() {
            if line.starts_with("btime ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 2 {
                    if let Ok(epoch) = parts[1].parse::<u64>() {
                        if let Ok(out) = Command::new("date").args(["-d", &format!("@{}", epoch), "+%Y-%m-%d %H:%M:%S"]).output() {
                            let time_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                            if !time_str.is_empty() {
                                return time_str;
                            }
                        }
                    }
                }
            }
        }
    }
    "Unknown".to_string()
}

fn get_default_gateway() -> String {
    if let Ok(out) = Command::new("ip").args(["route", "show", "default"]).output() {
        let output_str = String::from_utf8_lossy(&out.stdout);
        for part in output_str.split_whitespace() {
            if let Ok(addr) = part.parse::<std::net::IpAddr>() {
                if addr.is_ipv4() {
                    return addr.to_string();
                }
            }
        }
    }
    "N/A".to_string()
}

fn get_clean_listening_ports() -> Vec<String> {
    let mut result = Vec::new();
    if let Ok(out) = Command::new("ss").args(["-4", "-tlnp"]).output() {
        let output_str = String::from_utf8_lossy(&out.stdout);
        for line in output_str.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 4 {
                let local_address = fields[3];
                
                let mut proc_info = "unknown".to_string();
                if let Some(&last_field) = fields.last() {
                    if last_field.contains("users:(") {
                        proc_info = last_field
                            .replace("users:((\"", "")
                            .split('"')
                            .next()
                            .unwrap_or("unknown")
                            .to_string();
                        
                        if let Some(pid_pos) = last_field.find("pid=") {
                            let pid_str: String = last_field[pid_pos + 4..]
                                .chars()
                                .take_while(|c| c.is_ascii_digit())
                                .collect();
                            if !pid_str.is_empty() {
                                proc_info = format!("{} (PID {})", proc_info, pid_str);
                            }
                        }
                    }
                }

                result.push(format!("  [LISTEN]  {:<22} | Proc: {}", local_address, proc_info));
            }
        }
    }
    
    if result.is_empty() {
        vec!["  No listening IPv4 ports found".to_string()]
    } else {
        result
    }
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    dmesg_lines: &mut Vec<String>,
    search: &mut String,
    scroll_y: &mut usize,
    scroll_x: &mut usize,
    port_scroll: &mut usize,
    auto_scroll: &mut bool,
    show_info_panel: &mut bool,
    show_help_modal: &mut bool,
    active_filter: &mut LogFilter,
    log_source: &mut LogSource,
    export_status: &mut Option<String>,
    export_timer: &mut Option<Instant>,
    tx: &std::sync::mpsc::Sender<Message>,
    rx: &std::sync::mpsc::Receiver<Message>,
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    let boot_time_str = get_exact_boot_title();
    let gateway_ip = get_default_gateway();
    let mut view_state = LogViewState::new();
    let mut telemetry_timer = Instant::now();

    loop {
        if let Some(timer) = *export_timer {
            if timer.elapsed() >= Duration::from_secs(3) {
                *export_status = None;
                *export_timer = None;
            }
        }

        while let Ok(msg) = rx.try_recv() {
            match msg {
                Message::LogsUpdated(new_lines) => {
                    *dmesg_lines = new_lines;
                }
            }
        }

        if last_refresh.elapsed() >= refresh_interval {
            spawn_log_fetcher(tx.clone(), *log_source);
            *last_refresh = Instant::now();
        }

        if telemetry_timer.elapsed() >= Duration::from_secs(1) {
            sys.refresh_cpu();
            sys.refresh_memory();
            nets.refresh();
            telemetry_timer = Instant::now();
        }

        view_state.update_if_needed(dmesg_lines, search, *active_filter);

        let terminal_size = terminal.size()?;
        let current_chunks = if *show_info_panel {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Control Panel
                    Constraint::Length(3), // Search Bar
                    Constraint::Min(6),    // Logs
                    Constraint::Length(7), // Dashboard
                    Constraint::Length(5), // Ports
                ])
                .split(terminal_size)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Control Panel
                    Constraint::Length(3), // Search Bar
                    Constraint::Min(6),    // Logs
                ])
                .split(terminal_size)
        };
        let list_height = current_chunks[2].height.saturating_sub(2) as usize;

        terminal.draw(|f| {
            let chunks = current_chunks;

            let auto_status = if *auto_scroll { "ON" } else { "OFF" };
            let source_str = match log_source {
                LogSource::Dmesg => "dmesg",
                LogSource::Journalctl => "journalctl",
            };

            let f2_label = if *active_filter == LogFilter::Warn { "[F2:WARN*]" } else { "F2:Warn" };
            let f3_label = if *active_filter == LogFilter::Info { "[F3:INFO*]" } else { "F3:Info" };
            let f4_label = if *active_filter == LogFilter::Err { "[F4:ERR*]" } else { "F4:Err" };
            let f5_label = if *active_filter == LogFilter::Notice { "[F5:NOTICE*]" } else { "F5:Notice" };
            let all_label = if *active_filter == LogFilter::All { "[ALL]" } else { "All" };

            let filtered_count = view_state.filtered_indices.len();
            let max_scroll = filtered_count.saturating_sub(list_height);

            if *auto_scroll {
                *scroll_y = max_scroll;
            } else if *scroll_y > max_scroll {
                *scroll_y = max_scroll;
            }

            let start_line = if filtered_count == 0 { 0 } else { *scroll_y + 1 };
            let end_line = (*scroll_y + list_height).min(filtered_count);

            let top_text = format!(
                "Source: {} | Showing: {}-{} of {} matches (Total: {}) | Filters: {} {} {} {} {} | Auto: {}",
                source_str, start_line, end_line, filtered_count, dmesg_lines.len(), all_label, f2_label, f3_label, f4_label, f5_label, auto_status
            );
            f.render_widget(
                Paragraph::new(top_text)
                    .block(Block::default().borders(Borders::ALL).title("Control Panel & Counters").style(Style::default().fg(Color::Cyan))),
                chunks[0],
            );

            let search_display = if search.is_empty() {
                "Type to search logs...".to_string()
            } else {
                search.clone()
            };
            let search_color = if search.is_empty() { Color::DarkGray } else { Color::Yellow };
