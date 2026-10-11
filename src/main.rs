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

    let (tx, rx): (std::sync::mpsc::Sender<Message>, std::sync::mpsc::Receiver<Message>) = std::sync::mpsc::channel();

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

fn spawn_log_fetcher(tx: std::sync::mpsc::Sender<Message>, source: LogSource) {
    std::thread::spawn(move || {
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
                            if !time_str.is_
