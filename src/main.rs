use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
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
    let mut active_filter = LogFilter::All;

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);

    update_dmesg_buffer(&mut dmesg_lines, &mut scroll_y, auto_scroll, 20);

    let res = run_app(
        &mut terminal,
        &mut dmesg_lines,
        &mut search,
        &mut scroll_y,
        &mut scroll_x,
        &mut port_scroll,
        &mut auto_scroll,
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
        let new_lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.to_string())
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

fn get_boot_time_string(uptime_seconds: u64) -> String {
    if let Ok(duration_since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        let current_epoch = duration_since_epoch.as_secs();
        if current_epoch >= uptime_seconds {
            let boot_epoch = current_epoch - uptime_seconds;
            if let Ok(out) = Command::new("date").args(["-d", &format!("@{}", boot_epoch), "+%Y-%m-%d %H:%M:%S"]).output() {
                let time_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !time_str.is_empty() {
                    return time_str;
                }
            }
        }
    }
    "Unknown".to_string()
}

fn get_clean_listening_ports() -> Vec<String> {
    let mut result = Vec::new();
    if let Ok(out) = Command::new("ss").args(["-tln"]).output() {
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
        vec!["  No listening ports found".to_string()]
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
    active_filter: &mut LogFilter,
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    loop {
        let size = terminal.size()?;
        let list_height = if size.height > 20 { (size.height - 16) as usize } else { 8 };

        if last_refresh.elapsed() >= refresh_interval {
            update_dmesg_buffer(dmesg_lines, scroll_y, *auto_scroll, list_height);
            *last_refresh = Instant::now();
        }

        sys.refresh_all();
        nets.refresh();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Control Panel
                    Constraint::Min(8),     // Dmesg List
                    Constraint::Length(6),  // System / RAM / Swap / Nets
                    Constraint::Length(6),  // Ports List
                ])
                .split(f.size());

            // 1. Steuerungsleiste
            let auto_status = if *auto_scroll { "ON" } else { "OFF" };
            let filter_str = match active_filter {
                LogFilter::All => "ALL",
                LogFilter::Warn => "WARN (F2)",
                LogFilter::Info => "INFO (F3)",
                LogFilter::Err => "ERR (F4)",
                LogFilter::Notice => "NOTICE (F6)",
            };
            let top_text = format!(
                "Search: {} | Filter: {} | Auto: {} | [q]uit [r]otate [Ctrl+↑/↓]ports",
                search, filter_str, auto_status
            );
            f.render_widget(
                Paragraph::new(top_text)
                    .block(Block::default().borders(Borders::ALL).title("Control Panel")),
                chunks[0],
            );

            // 2. Dmesg Liste
            let filtered_lines: Vec<&String> = dmesg_lines
                .iter()
                .filter(|l| {
                    let matches_search = l.contains(&*search);
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
                    ListItem::new(truncated)
                })
                .collect();
            
            f.render_widget(
                List::new(display_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("dmesg (Current Boot - Shown: {}/{})", filtered_lines.len(), dmesg_lines.len()))
                ),
                chunks[1],
            );

            // 3. System-, RAM/Swap & Networks
            let os_name = System::name().unwrap_or_else(|| "Linux".to_string());
            let os_ver = System::os_version().unwrap_or_default();
            let distro = format!("{} {}", os_name, os_ver);
            
            let up_secs = System::uptime();
            let uptime_str = format_uptime(up_secs);
            let boot_time_str = get_boot_time_string(up_secs);
            
            let total_ram = sys.total_memory();
            let used_ram = sys.used_memory();
            let ram_bar = render_bar(used_ram, total_ram, 15);

            let total_swap = sys.total_swap();
            let used_swap = sys.used_swap();
            let swap_bar = render_bar(used_swap, total_swap, 15);

            let net_str: String = nets
                .iter()
                .map(|(n, i)| format!("{}: ↓{} ↑{}", n, i.received(), i.transmitted()))
                .collect::<Vec<_>>()
                .join(" | ");

            let sys_info_text = format!(
                "OS: {} | Boot: {} | Uptime: {}\nRAM:  {} ({}/{}) MB\nSwap: {} ({}/{}) MB\nNets: {}",
                distro,
                boot_time_str,
                uptime_str,
                ram_bar, used_ram / 1024 / 1024, total_ram / 1024 / 1024,
                swap_bar, used_swap / 1024 / 1024, total_swap / 1024 / 1024,
                if net_str.is_empty() { "No active interfaces".to_string() } else { net_str }
            );

            f.render_widget(
                Paragraph::new(sys_info_text).block(
                    Block::default().borders(Borders::ALL).title("System & Hardware Status")
                ),
                chunks[2],
            );

            // 4. Listening Ports ASCII Liste (Ctrl + Up/Down scrollbar)
            let ports_vec = get_clean_listening_ports();
            let ports_items: Vec<ListItem> = ports_vec
                .iter()
                .skip(*port_scroll)
                .map(|p| ListItem::new(p.as_str()))
                .collect();

            f.render_widget(
                List::new(ports_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("Listening Ports (Scroll: Ctrl+↑/↓ - Total: {})", ports_vec.len()))
                ),
                chunks[3],
            );
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) => match (k.modifiers, k.code) {
                    (KeyModifiers::CONTROL, KeyCode::Up) => {
                        if *port_scroll > 0 {
                            *port_scroll -= 1;
                        }
                    }
                    (KeyModifiers::CONTROL, KeyCode::Down) => {
                        *port_scroll += 1;
                    }
                    (_, KeyCode::Char('q')) => break,
                    (_, KeyCode::Char('r')) => {
                        update_dmesg_buffer(dmesg_lines, scroll_y, *auto_scroll, list_height);
                    }
                    (_, KeyCode::F(2)) => {
                        *active_filter = if *active_filter == LogFilter::Warn { LogFilter::All } else { LogFilter::Warn };
                        *scroll_y = 0;
                    }
                    (_, KeyCode::F(3)) => {
                        *active_filter = if *active_filter == LogFilter::Info { LogFilter::All } else { LogFilter::Info };
                        *scroll_y = 0;
                    }
                    (_, KeyCode::F(4)) => {
                        *active_filter = if *active_filter == LogFilter::Err { LogFilter::All } else { LogFilter::Err };
                        *scroll_y = 0;
                    }
                    (_, KeyCode::F(6)) => {
                        *active_filter = if *active_filter == LogFilter::Notice { LogFilter::All } else { LogFilter::Notice };
                        *scroll_y = 0;
                    }
                    (_, KeyCode::Char(c)) => {
                        search.push(c);
                        *scroll_y = 0;
                        *auto_scroll = false;
                    }
                    (_, KeyCode::Backspace) => {
                        search.pop();
                        *scroll_y = 0;
                        *auto_scroll = false;
                    }
                    (_, KeyCode::Up) => {
                        *auto_scroll = false;
                        if *scroll_y > 0 {
                            *scroll_y -= 1;
                        }
                    }
                    (_, KeyCode::Down) => {
                        *scroll_y += 1;
                        if *scroll_y + list_height >= dmesg_lines.len() {
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::PageUp) => {
                        *auto_scroll = false;
                        if *scroll_y >= list_height {
                            *scroll_y -= list_height;
                        } else {
                            *scroll_y = 0;
                        }
                    }
                    (_, KeyCode::PageDown) => {
                        *scroll_y += list_height;
                        if *scroll_y + list_height >= dmesg_lines.len() {
                            *scroll_y = dmesg_lines.len().saturating_sub(list_height);
                            *auto_scroll = true;
                        }
                    }
                    (_, KeyCode::Left) => {
                        if *scroll_x > 0 {
                            *scroll_x -= 3;
                        }
                    }
                    (_, KeyCode::Right) => {
                        *scroll_x += 3;
                    }
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        if m.modifiers.contains(KeyModifiers::ALT) {
                            if *scroll_x >= 3 { *scroll_x -= 3; } else { *scroll_x = 0; }
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
                        if *scroll_x >= 3 { *scroll_x -= 3; } else { *scroll_x = 0; }
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
