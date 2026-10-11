use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use regex::Regex;
use std::io;
use std::fs;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
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

#[derive(PartialEq, Clone, Copy)]
enum LogSource {
    Dmesg,
    Journalctl,
}

enum Message {
    LogsUpdated(Vec<String>),
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
    let mut log_source = LogSource::Dmesg;
    let mut export_status: Option<String> = None;

    let (tx, rx): (Sender<Message>, Receiver<Message>) = mpsc::channel();

    spawn_log_fetcher(tx.clone(), log_source);

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(3);

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

fn spawn_log_fetcher(tx: Sender<Message>, source: LogSource) {
    thread::spawn(move || {
        let lines = match source {
            LogSource::Dmesg => {
                if let Ok(out) = Command::new("dmesg").args(["-k", "-T"]).output() {
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
                if let Ok(out) = Command::new("journalctl").args(["-b", "-o", "short-iso", "-n", "2000"]).output() {
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
    log_source: &mut LogSource,
    export_status: &mut Option<String>,
    tx: &Sender<Message>,
    rx: &Receiver<Message>,
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    let boot_time_str = get_exact_boot_time();
    let gateway_ip = get_default_gateway();

    loop {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Message::LogsUpdated(new_lines) => {
                    let old_len = dmesg_lines.len();
                    *dmesg_lines = new_lines;
                    if *auto_scroll && dmesg_lines.len() > old_len {
                        *scroll_y = dmesg_lines.len().saturating_sub(1);
                    }
                }
            }
        }

        let size = terminal.size()?;
        // Nochmals 2 Zeilen weniger für dmesg (insgesamt 4 weniger als zuvor), damit unten maximal Platz ist
        let list_height = if *show_info_panel {
            if size.height > 28 { (size.height - 22) as usize } else { 4 }
        } else {
            (size.height.saturating_sub(6)) as usize
        };

        if last_refresh.elapsed() >= refresh_interval {
            spawn_log_fetcher(tx.clone(), *log_source);
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
                        Constraint::Min(4),
                        Constraint::Length(8),
                        Constraint::Length(5),
                    ])
                    .split(f.size())
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(6),
                    ])
                    .split(f.size())
            };

            let auto_status = if *auto_scroll { "ON" } else { "OFF" };
            let source_str = match log_source {
                LogSource::Dmesg => "dmesg",
                LogSource::Journalctl => "journalctl",
            };
            let filter_str = match active_filter {
                LogFilter::All => "ALL",
                LogFilter::Warn => "WARN (F2)",
                LogFilter::Info => "INFO (F3)",
                LogFilter::Err => "ERR (F4)",
                LogFilter::Notice => "NOTICE (F6)",
            };
            let top_text = format!(
                "Source: {} | Search: {} | Filter: {} | Auto: {} | [Ctrl+q]uit [Ctrl+r]otate [Ctrl+j]source [Ctrl+s]export [F1/Ctrl+h]help",
                source_str, search, filter_str, auto_status
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
            
            let title_suffix = if let Some(ref msg) = export_status {
                format!(" | {} ", msg)
            } else {
                "".to_string()
            };

            f.render_widget(
                List::new(display_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("Logs ({}) - Shown: {}/{}{}", source_str, filtered_lines.len(), dmesg_lines.len(), title_suffix))
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
                    "OS: {} | Started: {} | Uptime: {}\nRAM:  {} ({}/{}) MB | Swap: {} ({}/{}) MB\nGateway (IPv4): {}\n{}",
                    distro,
                    boot_time_str,
                    uptime_str,
                    ram_bar, used_ram / 1024 / 1024, total_ram / 1024 / 1024,
                    swap_bar, used_swap / 1024 / 1024, total_swap / 1024 / 1024,
                    gateway_ip,
                    net_display
                );

                f.render_widget(
                    Paragraph::new(sys_info_text).block(
                        Block::default().borders(Borders::ALL).title("System & Network Dashboard (IPv4 Only)").style(Style::default().fg(Color::Cyan))
                    ),
                    chunks[2],
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
                            .title(format!("Listening IPv4 Ports (Ctrl+↑/↓ - Total: {})", ports_vec.len()))
                            .style(Style::default().fg(Color::Cyan))
                    ),
                    chunks[3],
                );
            }

            // Hilfe-Modal Overlay (Weißer Text auf blauem Hintergrund)
            if *show_help_modal {
                let area = f.size();
                let popup_area = Rect {
                    x: area.width / 4,
                    y: area.height / 5,
                    width: area.width / 2,
                    height: (area.height * 3) / 5,
                };
                let help_text = vec![
                    Line::from("=== KDMSG-TUI-NG HOTKEYS ==="),
                    Line::from(""),
                    Line::from("Ctrl + q         : Beenden (Quit)"),
                    Line::from("Ctrl + r         : Logs neu laden (Rotate)"),
                    Line::from("Ctrl + j         : Log-Quelle wechseln (dmesg / journalctl)"),
                    Line::from("Ctrl + a         : Auto-Scroll ein/ausschalten"),
                    Line::from("Ctrl + i         : Info-Panel ein/ausblenden"),
                    Line::from("Ctrl + s         : Gefilterte Logs exportieren"),
                    Line::from("F1 / Ctrl + h    : Hilfe-Ansicht umschalten / Schließen"),
                    Line::from("Esc              : Hilfe-Ansicht schließen"),
                    Line::from("Tab / Shift+Tab  : Zu nächster / vorheriger Warnung/Fehler springen"),
                    Line::from("F2 / F3 / F4     : Filter-Presets (Warn, Info, Err)"),
                    Line::from("Ctrl + ↑ / ↓     : Ports-Liste scrollen"),
                ];
                let help_paragraph = Paragraph::new(help_text)
                    .style(Style::default().fg(Color::White).bg(Color::Blue))
                    .block(Block::default().borders(Borders::ALL).title(" Hilfe & Shortcuts ").style(Style::default().fg(Color::White).bg(Color::Blue)))
                    .alignment(Alignment::Center);
                
                f.render_widget(Clear, popup_area);
                f.render_widget(help_paragraph, popup_area);
            }
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) => match (k.modifiers, k.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('q')) => break,
                    (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                        *auto_scroll = true;
                        spawn_log_fetcher(tx.clone(), *log_source);
                    }
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
                        if *show_help_modal {
                            *show_help_modal = false;
                        }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                        let filtered_lines: Vec<String> = dmesg_lines
                            .iter()
                            .filter(|l| l.contains(&*search))
                            .cloned()
                            .collect();
                        if fs::write("kdmsg_export.txt", filtered_lines.join("\n")).is_ok() {
                            *export_status = Some("[Exported!]".to_string());
                        } else {
                            *export_status = Some("[Export Failed!]".to_string());
                        }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Up) => {
                        if *port_scroll > 0 { *port_scroll -= 1; }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Down) => {
                        *port_scroll += 1;
                    }
                    (KeyModifiers::NONE, KeyCode::Tab) => {
                        *auto_scroll = false;
                        if let Some(pos) = dmesg_lines.iter().enumerate().skip(*scroll_y + 1).find(|(_, l)| {
                            let low = l.to_lowercase();
                            low.contains("warn") || low.contains("err") || low.contains("fail")
                        }) {
                            *scroll_y = pos.0;
                        }
                    }
                    (KeyModifiers::SHIFT, KeyCode::BackTab) | (KeyModifiers::CONTROL, KeyCode::BackTab) => {
                        *auto_scroll = false;
                        if *scroll_y > 0 {
                            if let Some(pos) = dmesg_lines.iter().enumerate().take(*scroll_y).rev().find(|(_, l)| {
                                let low = l.to_lowercase();
                                low.contains("warn") || low.contains("err") || low.contains("fail")
                            }) {
                                *scroll_y = pos.0;
                            }
                        }
                    }
                    (_, KeyCode::F(2)) => {
                        *active_filter = if *active_filter == LogFilter::Warn { LogFilter::All } else { LogFilter::Warn };
                        *auto_scroll = true;
                    }
                    (_, KeyCode::F(3)) => {
                        *active_filter = if *active_filter == LogFilter::Info { LogFilter::All } else { LogFilter::Info };
                        *auto_scroll = true;
                    }
                    (_, KeyCode::F(4)) => {
                        *active_filter = if *active_filter == LogFilter::Err { LogFilter::All } else { LogFilter::Err };
                        *auto_scroll = true;
                    }
                    (_, KeyCode::F(6)) => {
                        *active_filter = if *active_filter == LogFilter::Notice { LogFilter::All } else { LogFilter::Notice };
                        *auto_scroll = true;
                    }
                    (_, KeyCode::Char(c)) => {
                        if !*show_help_modal {
                            search.push(c);
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::Backspace) => {
                        if !*show_help_modal {
                            search.pop();
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::Up) => {
                        *auto_scroll = false;
                        if *scroll_y > 0 { *scroll_y -= 1; }
                    }
                    (_, KeyCode::Down) => {
                        *scroll_y += 1;
                        if *scroll_y + list_height >= dmesg_lines.len() {
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::PageUp) => {
                        *auto_scroll = false;
                        *scroll_y = scroll_y.saturating_sub(list_height);
                    }
                    (_, KeyCode::PageDown) => {
                        *scroll_y += list_height;
                        if *scroll_y + list_height >= dmesg_lines.len() {
                            *scroll_y = dmesg_lines.len().saturating_sub(list_height);
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::Left) => {
                        *scroll_x = scroll_x.saturating_sub(3);
                    }
                    (_, KeyCode::Right) => {
                        *scroll_x += 3;
                    }
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        if m.modifiers.contains(KeyModifiers::ALT) {
                            *scroll_x = scroll_x.saturating_sub(3);
                        } else {
                            *auto_scroll = false;
                            if *scroll_y > 0 { *scroll_y -= 1; }
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if m.modifiers.contains(KeyModifiers::ALT) {
                            *scroll_x += 3;
                        } else {
                            *scroll_y += 1;
                            if *scroll_y + list_height >= dmesg_lines.len() {
                                *auto_scroll = true;
                            }
                        }
                    }
                    MouseEventKind::ScrollLeft => {
                        *scroll_x = scroll_x.saturating_sub(3);
                    }
                    MouseEventKind::ScrollRight => {
                        *scroll_x += 3;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
