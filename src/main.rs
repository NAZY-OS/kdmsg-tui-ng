use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use regex::Regex;
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};

#[derive(PartialEq, Clone, Copy)]
enum LogFilter {
    All,
    Warn,
    Info,
    Err,
    Notice,
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut dmesg_lines: Vec<String> = Vec::with_capacity(5000);
    let mut search = String::new();
    let mut scroll_y: usize = 0;
    let mut scroll_x: usize = 0;
    let mut port_scroll: usize = 0;
    let mut auto_scroll = true;
    let mut show_info_panel = true;
    let mut show_help_modal = false;
    let mut active_filter = LogFilter::All;

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);

    update_dmesg_buffer(&mut dmesg_lines, &mut scroll_y, auto_scroll, 12);

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

fn update_dmesg_buffer(dmesg_lines: &mut Vec<String>, scroll_y: &mut usize, auto_scroll: bool, page_size: usize) {
    if let Ok(out) = Command::new("dmesg").args(["-k", "-T"]).output() {
        let raw_output = String::from_utf8_lossy(&out.stdout);
        let year_regex = Regex::new(r" \d{4}\]").unwrap();

        let new_lines: Vec<String> = raw_output
            .lines()
            .map(|s| year_regex.replace(s, "]").to_string())
            .collect();
        
        let old_len = dmesg_lines.len();
        *dmesg_lines = new_lines;

        if auto_scroll && dmesg_lines.len() > old_len {
            if dmesg_lines.len() > page_size {
                *scroll_y = dmesg_lines.len() - page_size;
            } else {
                *scroll_y = 0;
            }
        }
    }
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

fn get_exact_boot_time() -> String {
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
    if let Ok(out) = Command::new("ss").args(["-4", "-tln"]).output() {
        let output_str = String::from_utf8_lossy(&out.stdout);
        for line in output_str.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 4 {
                let local_address = fields[3];
                result.push(format!("  [LISTEN]  {}", local_address));
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
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    let boot_time_str = get_exact_boot_time();
    let gateway_ip = get_default_gateway();

    loop {
        if last_refresh.elapsed() >= refresh_interval {
            update_dmesg_buffer(dmesg_lines, scroll_y, *auto_scroll, 12);
            *last_refresh = Instant::now();
        }

        sys.refresh_all();
        nets.refresh();

        terminal.draw(|f| {
            let chunks = if *show_info_panel {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(0),
                        Constraint::Length(7),
                        Constraint::Length(5),
                    ])
                    .split(f.size())
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(0),
                    ])
                    .split(f.size())
            };

            let list_height = chunks[1].height.saturating_sub(2) as usize;

            let auto_status = if *auto_scroll { "ON" } else { "OFF" };
            let filter_str = match active_filter {
                LogFilter::All => "ALL",
                LogFilter::Warn => "WARN (F2)",
                LogFilter::Info => "INFO (F3)",
                LogFilter::Err => "ERR (F4)",
                LogFilter::Notice => "NOTICE (F6)",
            };
            let top_text = format!(
                "Search: {} | Filter: {} | Auto: {} | [Ctrl+q]uit [Ctrl+r]otate [Ctrl+a]uto [Ctrl+i]info [F1]help",
                search, filter_str, auto_status
            );
            f.render_widget(
                Paragraph::new(top_text)
                    .block(Block::default().borders(Borders::ALL).title("Control Panel").style(Style::default().fg(Color::Cyan))),
                chunks[0],
            );

            let search_regex = Regex::new(search).ok();

            let filtered_lines: Vec<&String> = dmesg_lines
                .iter()
                .filter(|l| {
                    let matches_search = if let Some(ref reg) = search_regex {
                        reg.is_match(l)
                    } else {
                        l.contains(&*search)
                    };

                    let matches_filter = match active_filter {
                        LogFilter::All => true,
                        LogFilter::Warn => l.to_lowercase().contains("warn"),
                        LogFilter::Info => l.to_lowercase().contains("info"),
                        LogFilter::Err => l.to_lowercase().contains("err") || l.to_lowercase().contains("fail"),
                        LogFilter::Notice => l.to_lowercase().contains("notice"),
                    };
                    matches_search && matches_filter
                })
                .collect();

            if *auto_scroll && !filtered_lines.is_empty() {
                if filtered_lines.len() > list_height {
                    *scroll_y = filtered_lines.len() - list_height;
                } else {
                    *scroll_y = 0;
                }
            }

            let display_items: Vec<ListItem> = filtered_lines
                .iter()
                .skip(*scroll_y)
                .take(list_height)
                .map(|l| {
                    let truncated = if *scroll_x < l.len() {
                        &l[*scroll_x..]
                    } else {
                        ""
                    };

                    let lower = l.to_lowercase();
                    let (fg_color, bg_color) = if lower.contains("err") || lower.contains("fail") || lower.contains("crit") {
                        (Color::Red, Color::Black)
                    } else if lower.contains("warn") {
                        (Color::Yellow, Color::Black)
                    } else {
                        (Color::Green, Color::Black)
                    };

                    ListItem::new(truncated).style(Style::default().fg(fg_color).bg(bg_color))
                })
                .collect();
            
            f.render_widget(
                List::new(display_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("dmesg (Current Boot - Shown: {}/{})", filtered_lines.len(), dmesg_lines.len()))
                        .style(Style::default().fg(Color::Cyan))
                ),
                chunks[1],
            );

            if *show_info_panel && chunks.len() > 3 {
                let os_name = System::name().unwrap_or_else(|| "Linux".to_string());
                let os_ver = System::os_version().unwrap_or_default();
                let distro = format!("{} {}", os_name, os_ver);
                
                let up_secs = System::uptime();
                let uptime_str = format_uptime(up_secs);
                
                let total_ram = sys
