use crossterm::{event::{self, Event, KeyCode, MouseEventKind}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{prelude::*, widgets::*};
use std::io;
use std::process::Command;
use std::time::Duration;
use sysinfo::{Networks, System};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut dmesg_lines: Vec<String> = Vec::with_capacity(5000);
    let mut search = String::new();
    let mut scroll: usize = 0;
    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();

    loop {
        if let Ok(out) = Command::new("dmesg").output() {
            let new_lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|s| s.to_string()).collect();
            for line in new_lines { if dmesg_lines.len() < 5000 { dmesg_lines.push(line); } }
        }

        sys.refresh_all();
        nets.refresh();

        terminal.draw(|f| {
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(12), Constraint::Length(10)]).split(f.area());
            f.render_widget(Paragraph::new(format!("Search: {} | q=quit", search)), chunks[0]);

            let filtered: Vec<ListItem> = dmesg_lines.iter().filter(|l| l.contains(&search)).skip(scroll).take(20).map(|l| ListItem::new(l.as_str())).collect();
            f.render_widget(List::new(filtered).block(Block::bordered().title("dmesg (5000 lines)")), chunks[1]);

            let net_str: String = nets.iter().map(|(n,i)| format!("{} rx/tx {}/{}", n, i.received(), i.transmitted())).collect::<Vec<_>>().join(" | ");
            let ports = std::fs::read_to_string("/proc/net/tcp").unwrap_or_default();
            let info = format!("Uptime: {}s | CPU: {}% | Mem: {}MB\nNets: {}\nOpen/Listen: {}", sys.uptime(), sys.global_cpu_usage(), sys.total_memory()/1024/1024, net_str, ports.lines().take(8).collect::<Vec<_>>().join("\n"));
            f.render_widget(Paragraph::new(info).block(Block::bordered().title("sys + ports")), chunks[2]);
        })?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(k) = event::read()? {
                match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(c) => search.push(c),
                    KeyCode::Backspace => { search.pop(); },
                    KeyCode::Up => { if scroll > 0 { scroll -= 1; } },
                    KeyCode::Down => { scroll += 1; },
                    _ => {}
                }
            } else if let Event::Mouse(m) = event::read()? {
                if let MouseEventKind::ScrollUp = m.kind { if scroll > 0 { scroll -= 1; } }
                if let MouseEventKind::ScrollDown = m.kind { scroll += 1; }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, event::DisableMouseCapture)?;
    Ok(())
