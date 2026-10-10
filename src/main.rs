use crossterm::{
    event::{self, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut dmesg_lines: Vec<String> = Vec::with_capacity(5000);
    let mut search = String::new();
    let mut scroll: usize = 0;
    let mut auto_scroll = true;

    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);

    update_dmesg_buffer(&mut dmesg_lines, &mut scroll, auto_scroll);

    let res = run_app(
        &mut terminal,
        &mut dmesg_lines,
        &mut search,
        &mut scroll,
        &mut auto_scroll,
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

/// Lädt dmesg mit human-readable timestamps (-T) und Kernel-only (-k)
fn update_dmesg_buffer(dmesg_lines: &mut Vec<String>, scroll: &mut usize, auto_scroll: bool) {
    if let Ok(out) = Command::new("dmesg").args(["-k", "-T"]).output() {
        let new_lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
        
        let old_len = dmesg_lines.len();
        *dmesg_lines = new_lines;

        if auto_scroll && dmesg_lines.len() > old_len {
            if dmesg_lines.len() > 20 {
                *scroll = dmesg_lines.len() - 20;
            } else {
                *scroll = 0;
            }
        }
    }
}

/// Generiert einen ASCII-Fortschrittsbalken
fn render_bar(current: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return "[--------------------]".to_string();
    }
    let filled = ((current as f64 / total as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!("[{}{}] {:.1}%", "█".repeat(filled), "-".repeat(empty), (current as f64 / total as f64) * 100.0)
}

/// Formatiert Sekunden in ein lesbares Format
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

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    dmesg_lines: &mut Vec<String>,
    search: &mut String,
    scroll: &mut usize,
    auto_scroll: &mut bool,
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    loop {
        if last_refresh.elapsed() >= refresh_interval {
            update_dmesg_buffer(dmesg_lines, scroll, *auto_scroll);
            *last_refresh = Instant::now();
        }

        sys.refresh_all();
        nets.refresh();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(12),
                    Constraint::Length(11),
                ])
                .split(f.size());

            // 1. Suchleiste
            let auto_status = if *auto_scroll { "ON" } else { "OFF (Manual Scroll)" };
            f.render_widget(
                Paragraph::new(format!("Search: {} | Auto-Scroll: {} | q=quit", search, auto_status))
                    .block(Block::default().borders(Borders::ALL).title("Filter & Control")),
                chunks[0],
            );

            // 2. Dmesg Liste
            let filtered: Vec<ListItem> = dmesg_lines
                .iter()
                .filter(|l| l.contains(&*search))
                .skip(*scroll)
                .take(20)
                .map(|l| ListItem::new(l.as_str()))
                .collect();
            
            f.render_widget(
                List::new(filtered).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("dmesg (Human Timestamps - Total: {})", dmesg_lines.len()))
                ),
                chunks[1],
            );

            // 3. System- & Netzwerk-Infos mit Graphen
            let os_name = System::name().unwrap_or_else(|| "Linux".to_string());
            let os_ver = System::os_version().unwrap_or_default();
            let distro = format!("{} {}", os_name, os_ver);
            let uptime = format_uptime(System::uptime());
            
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

            let ports = std::fs::read_to_string("/proc/net/tcp").unwrap_or_default();
            let listen_ports: Vec<String> = ports
                .lines()
                .skip(1)
                .take(3)
                .map(|l| {
                    let _parts: Vec<&str> = l.split_whitespace().collect();
                    l.to_string()
                })
                .collect();

            let info = format!(
                "OS: {} | Uptime: {}\nRAM:  {} ({}/{}) MB\nSwap: {} ({}/{}) MB\nNets: {}\nPorts (Listen):\n{}",
                distro,
                uptime,
                ram_bar, used_ram / 1024 / 1024, total_ram / 1024 / 1024,
                swap_bar, used_swap / 1024 / 1024, total_swap / 1024 / 1024,
                if net_str.is_empty() { "No active interfaces".to_string() } else { net_str },
                if listen_ports.is_empty() { "None".to_string() } else { listen_ports.join("\n") }
            );

            f.render_widget(
                Paragraph::new(info).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("System, RAM/Swap & Ports")
                ),
                chunks[2],
            );
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) => match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(c) => {
                        search.push(c);
                        *scroll = 0;
                        *auto_scroll = false;
                    }
                    KeyCode::Backspace => {
                        search.pop();
                        *scroll = 0;
                        *auto_scroll = false;
                    }
                    KeyCode::Up => {
                        *auto_scroll = false;
                        if *scroll > 0 {
                            *scroll -= 1;
                        }
                    }
                    KeyCode::Down => {
                        *scroll += 1;
                        if *scroll + 20 >= dmesg_lines.len() {
                            *auto_scroll = true;
                        }
                    }
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        *auto_scroll = false;
                        if *scroll > 0 {
                            *scroll -= 1;
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        *scroll += 1;
                        if *scroll + 20 >= dmesg_lines.len() {
                            *auto_scroll = true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
