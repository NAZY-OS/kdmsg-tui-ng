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
    let refresh_interval = Duration::from_secs(2);

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
        let lines = match source {
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

        sys.refresh_all();
        nets.refresh();

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
            f.render_widget(
                Paragraph::new(search_display)
                    .style(Style::default().fg(search_color))
                    .block(Block::default().borders(Borders::ALL).title(" Search Filter ").style(Style::default().fg(Color::LightYellow))),
                chunks[1],
            );

            let display_items: Vec<ListItem> = view_state.filtered_indices
                .iter()
                .skip(*scroll_y)
                .take(list_height)
                .map(|&idx| {
                    let l = &dmesg_lines[idx];
                    let truncated = if *scroll_x < l.len() {
                        &l[*scroll_x..]
                    } else {
                        ""
                    };

                    let lower = l.to_lowercase();
                    let fg_color = if lower.contains("err") || lower.contains("fail") || lower.contains("crit") || lower.contains("error") || lower.contains("fault") || lower.contains("segfault") || lower.contains("oops") || lower.contains("denied") || lower.contains("<3>") || lower.contains("<2>") || lower.contains("<1>") || lower.contains("<0>") {
                        Color::Red
                    } else if lower.contains("warn") || lower.contains("warning") || lower.contains("bad") || lower.contains("degraded") || lower.contains("slow") || lower.contains("bug") || lower.contains("<4>") {
                        Color::Yellow
                    } else if lower.contains("info") || lower.contains("usb") || lower.contains("pci") || lower.contains("cpu") || lower.contains("acpi") || lower.contains("net") || lower.contains("registered") || lower.contains("<6>") || lower.contains("<5>") {
                        Color::Cyan
                    } else if lower.contains("notice") || lower.contains("note") || lower.contains("ready") || lower.contains("started") || lower.contains("version") || lower.contains("boot") || lower.contains("mount") {
                        Color::Magenta
                    } else {
                        Color::Green
                    };

                    ListItem::new(truncated).style(Style::default().fg(fg_color).bg(Color::Black))
                })
                .collect();
            
            let title_suffix = if let Some(ref msg) = export_status {
                format!(" | {} ", msg)
            } else {
                "".to_string()
            };

            f.render_widget(
                List::new(display_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(
                            " Logs Viewer [Active Filter: {:?}] {}", 
                            *active_filter,
                            title_suffix
                        ))
                        .style(Style::default().fg(Color::Cyan))
                ),
                chunks[2],
            );

            if *show_info_panel && chunks.len() > 3 {
                let os_name = System::name().unwrap_or_else(|| "Linux".to_string());
                let os_ver = System::os_version().unwrap_or_default();
                let distro = format!("{} {}", os_name, os_ver);
                
                let up_secs = System::uptime();
                let uptime_str = format_uptime(up_secs);
                
                // CPU Gesamtauslastung berechnen
                let global_cpu = sys.global_cpu_usage();
                let cpu_bar = render_bar(global_cpu.round() as u64, 100, 15);

                let total_ram = sys.total_memory();
                let used_ram = sys.used_memory();
                let ram_bar = render_bar(used_ram, total_ram, 15);

                let total_swap = sys.total_swap();
                let used_swap = sys.used_swap();
                let swap_bar = render_bar(used_swap, total_swap, 15);

                let mut net_info_lines = Vec::new();
                for (name, data) in nets.iter() {
                    let ips: Vec<String> = data.ip_networks()
                        .iter()
                        .filter(|ip| ip.addr.is_ipv4())
                        .map(|ip| ip.addr.to_string())
                        .collect();
                    let ip_str = if ips.is_empty() { "No IPv4".to_string() } else { ips.join(", ") };
                    
                    let down_kb = data.received() / 1024;
                    let up_kb = data.transmitted() / 1024;
                    
                    net_info_lines.push(format!("  [NET] {:<8} | IP: {:<15} | ↓ {:>4} KB/s  ↑ {:>4} KB/s", name, ip_str, down_kb, up_kb));
                }
                let net_display = if net_info_lines.is_empty() {
                    "  No networks found".to_string()
                } else {
                    net_info_lines.join("\n")
                };

                let sys_info_text = format!(
                    "OS: {} | Started: {} | Uptime: {}\nCPU:  {} ({:.1}%)\nRAM:  {} ({}/{}) MB | Swap: {} ({}/{}) MB\nGateway (IPv4): {}\n{}",
                    distro,
                    boot_time_str,
                    uptime_str,
                    cpu_bar, global_cpu,
                    ram_bar, used_ram / 1024 / 1024, total_ram / 1024 / 1024,
                    swap_bar, used_swap / 1024 / 1024, total_swap / 1024 / 1024,
                    gateway_ip,
                    net_display
                );

                f.render_widget(
                    Paragraph::new(sys_info_text).block(
                        Block::default().borders(Borders::ALL).title("System & Network Dashboard (IPv4 Only)").style(Style::default().fg(Color::Cyan))
                    ),
                    chunks[3],
                );

                let ports_vec = get_clean_listening_ports();
                let ports_items: Vec<ListItem> = ports_vec
                    .iter()
                    .skip(*port_scroll)
                    .map(|p| ListItem::new(p.as_str()).style(Style::default().fg(Color::LightCyan)))
                    .collect();

                f.render_widget(
                    List::new(ports_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(format!("Listening IPv4 Ports with Process/PID (Ctrl+↑/↓ - Total: {})", ports_vec.len()))
                            .style(Style::default().fg(Color::Cyan))
                    ),
                    chunks[4],
                );
            }

            if *show_help_modal {
                let area = f.size();
                let popup_width = 64.min(area.width.saturating_sub(4));
                let popup_height = 16.min(area.height.saturating_sub(4));
                let popup_area = Rect {
                    x: (area.width.saturating_sub(popup_width)) / 2,
                    y: (area.height.saturating_sub(popup_height)) / 2,
                    width: popup_width,
                    height: popup_height,
                };

                let help_text = vec![
                    Line::from(Span::styled(" KDMSG-TUI-NG HELP & SHORTCUTS ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from(vec![Span::styled(" Ctrl + q        ", Style::default().fg(Color::Green)), Span::raw(": Quit application")]),
                    Line::from(vec![Span::styled(" Ctrl + j        ", Style::default().fg(Color::Green)), Span::raw(": Toggle log source (dmesg / journalctl)")]),
                    Line::from(vec![Span::styled(" Ctrl + a        ", Style::default().fg(Color::Green)), Span::raw(": Toggle auto-scroll")]),
                    Line::from(vec![Span::styled(" Ctrl + i        ", Style::default().fg(Color::Green)), Span::raw(": Toggle info panel")]),
                    Line::from(vec![Span::styled(" Ctrl + s        ", Style::default().fg(Color::Green)), Span::raw(": Export filtered logs")]),
                    Line::from(vec![Span::styled(" F1 / Ctrl + h   ", Style::default().fg(Color::Green)), Span::raw(": Toggle help screen")]),
                    Line::from(vec![Span::styled(" F2..F5          ", Style::default().fg(Color::Green)), Span::raw(": Filter presets (Warn, Info, Err, Notice")]),
                    Line::from(vec![Span::styled(" Tab / Shift+Tab ", Style::default().fg(Color::Green)), Span::raw(": Jump next / previous warning")]),
                ];
                let help_paragraph = Paragraph::new(help_text)
                    .style(Style::default().fg(Color::White).bg(Color::Blue))
                    .block(Block::default().borders(Borders::ALL).title(" Help ").style(Style::default().fg(Color::White).bg(Color::Blue)))
                    .alignment(Alignment::Left);
                
                f.render_widget(Clear, popup_area);
                f.render_widget(help_paragraph, popup_area);
            }
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) => match (k.modifiers, k.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('q')) => break,
                    (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                        *log_source = match *log_source {
                            LogSource::Dmesg => LogSource::Journalctl,
                            LogSource::Journalctl => LogSource::Dmesg,
                        };
                        spawn_log_fetcher(tx.clone(), *log_source);
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                        *auto_scroll = !*auto_scroll;
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('i')) => {
                        *show_info_panel = !*show_info_panel;
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('h')) | (_, KeyCode::F(1)) => {
                        *show_help_modal = !*show_help_modal;
                    }
                    (_, KeyCode::Esc) => {
                        if *show_help_
