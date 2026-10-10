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
    load_dmesg(&mut dmesg_lines);

    let mut search = String::new();
    let mut scroll: usize = 0;
    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    // Timer für automatisches Aktualisieren (Rotieren) von dmesg
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(3); // Alle 3 Sekunden aktualisieren

    let res = run_app(
        &mut terminal,
        &mut dmesg_lines,
        &mut search,
        &mut scroll,
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

fn load_dmesg(dmesg_lines: &mut Vec<String>) {
    if let Ok(out) = Command::new("dmesg").arg("-T").output() {
        // -T aktiviert lesbare Zeitstempel (menschlich lesbare Zeit)
        *dmesg_lines = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
    }
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    dmesg_lines: &mut Vec<String>,
    search: &mut String,
    scroll: &mut usize,
    sys: &mut System,
    nets: &mut Networks,
    last_refresh: &mut Instant,
    refresh_interval: Duration,
) -> io::Result<()> {
    loop {
        // Automatisch dmesg und System-Infos rotieren/aktualisieren
        if last_refresh.elapsed() >= refresh_interval {
            load_dmesg(dmesg_lines);
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
                    Constraint::Length(10),
                ])
                .split(f.size());

            // 1. Suchleiste
            f.render_widget(
                Paragraph::new(format!("Search: {} | q=quit | Auto-refresh active", search))
                    .block(Block::default().borders(Borders::ALL).title("Filter")),
                chunks[0],
            );

            // 2. Dmesg Liste mit Zeitstempeln
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
                        .title(format!("dmesg mit Zeitstempeln (Total: {})", dmesg_lines.len()))
                ),
                chunks[1],
            );

            // 3. System- & Netzwerk-Infos
            let net_str: String = nets
                .iter()
                .map(|(n, i)| format!("{} rx/tx {}/{}", n, i.received(), i.transmitted()))
                .collect::<Vec<_>>()
                .join(" | ");

            let ports = std::fs::read_to_string("/proc/net/tcp").unwrap_or_default();
            let info = format!(
                "Uptime: {}s | CPU: {:.1}% | Mem: {}MB\nNets: {}\nOpen/Listen:\n{}",
                System::uptime(),
                sys.global_cpu_usage(),
                sys.total_memory() / 1024 / 1024,
                if net_str.is_empty() { "None".to_string() } else { net_str },
                ports.lines().take(5).collect::<Vec<_>>().join("\n")
            );

            f.render_widget(
                Paragraph::new(info).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("System & Ports")
                ),
                chunks[2],
            );
        })?;

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                Event::Key(k) => match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(c) => {
                        search.push(c);
                        *scroll = 0;
                    }
                    KeyCode::Backspace => {
                        search.pop();
                        *scroll = 0;
                    }
                    KeyCode::Up => {
                        if *scroll > 0 {
                            *scroll -= 1;
                        }
                    }
                    KeyCode::Down => {
                        *scroll += 1;
                    }
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        if *scroll > 0 {
                            *scroll -= 1;
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        *scroll += 1;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
