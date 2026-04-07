use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use super::theme::Gruvbox;
use crate::store::{Store, Watcher};
use crate::watcher::is_process_alive;

pub fn run(store: &Store) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, store);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, store: &Store) -> Result<()> {
    let mut watchers = load_watchers(store)?;
    let mut list_state = ListState::default();
    if !watchers.is_empty() {
        list_state.select(Some(0));
    }

    loop {
        terminal.draw(|f| render(f, f.area(), &watchers, &mut list_state))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => break,
                    KeyCode::Up | KeyCode::Char('k') => {
                        let len = watchers.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let len = watchers.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some((i + 1) % len));
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Char('s') => {
                        // Stop selected watcher
                        if let Some(i) = list_state.selected() {
                            if i < watchers.len() {
                                let _ = crate::watcher::stop_watcher(&watchers[i].id);
                                watchers = load_watchers(store)?;
                                if watchers.is_empty() {
                                    list_state.select(None);
                                } else if i >= watchers.len() {
                                    list_state.select(Some(watchers.len() - 1));
                                }
                            }
                        }
                    }
                    KeyCode::Char('X') => {
                        // Cleanup dead watchers
                        let _ = crate::watcher::cleanup_watchers();
                        watchers = load_watchers(store)?;
                        if watchers.is_empty() {
                            list_state.select(None);
                        } else {
                            list_state.select(Some(0));
                        }
                    }
                    KeyCode::Char('r') => {
                        // Refresh list
                        watchers = load_watchers(store)?;
                        if watchers.is_empty() {
                            list_state.select(None);
                        }
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        let len = watchers.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        let len = watchers.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some((i + 1) % len));
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

fn load_watchers(store: &Store) -> Result<Vec<Watcher>> {
    store.list_watchers()
}

fn status_display(w: &Watcher) -> (&str, ratatui::style::Color) {
    let alive = w.pid.map(|pid| is_process_alive(pid)).unwrap_or(false);
    match (w.status.as_str(), alive) {
        ("running", true) => ("▶ running", Gruvbox::GREEN),
        ("running", false) => ("✘ dead", Gruvbox::RED),
        ("completed", _) => ("✔ done", Gruvbox::AQUA),
        ("stopped", _) => ("⏹ stopped", Gruvbox::YELLOW),
        (s, _) => (s, Gruvbox::GRAY),
    }
}

fn type_icon(watcher_type: &str) -> (&str, ratatui::style::Color) {
    match watcher_type {
        "pipeline" => ("⚙", Gruvbox::BLUE),
        "pr-merge" => ("⎇", Gruvbox::PURPLE),
        "pr-comments" => ("💬", Gruvbox::YELLOW),
        "sonarqube" => ("🔍", Gruvbox::AQUA),
        "custom" => ("⚡", Gruvbox::ORANGE),
        _ => ("?", Gruvbox::GRAY),
    }
}

fn render(
    f: &mut Frame,
    area: Rect,
    watchers: &[Watcher],
    list_state: &mut ListState,
) {
    f.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // spacer
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(area);

    // Title
    let active = watchers.iter().filter(|w| {
        w.status == "running" && w.pid.map(|p| is_process_alive(p)).unwrap_or(false)
    }).count();
    let title_text = if watchers.is_empty() {
        "👁 Watchers".to_string()
    } else {
        format!("👁 Watchers  ({} active, {} total)", active, watchers.len())
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                title_text,
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[0],
    );

    // List
    if watchers.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No watchers running. Use 'tcs watch <type> --id <id>' to start one.",
                Style::default().fg(Gruvbox::GRAY),
            ))),
            chunks[2],
        );
    } else {
        let items: Vec<ListItem> = watchers
            .iter()
            .map(|w| {
                let (status_text, status_color) = status_display(w);
                let (icon, icon_color) = type_icon(&w.watcher_type);

                let pid_text = w.pid
                    .map(|p| format!("pid:{}", p))
                    .unwrap_or_else(|| "-".to_string());

                let time = &w.started_at[..16.min(w.started_at.len())];

                let spans = vec![
                    Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
                    Span::styled(
                        format!("{:<12}", w.watcher_type),
                        Style::default().fg(Gruvbox::FG_BRIGHT),
                    ),
                    Span::styled(
                        format!("{:<12}", status_text),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        format!("  {}  ", pid_text),
                        Style::default().fg(Gruvbox::DARK_GRAY),
                    ),
                    Span::styled(
                        format!("  {}  ", w.config),
                        Style::default().fg(Gruvbox::GRAY),
                    ),
                    Span::styled(time, Style::default().fg(Gruvbox::DARK_GRAY)),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        f.render_stateful_widget(list, chunks[2], list_state);
    }

    // Footer
    let help = vec![
        Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
        Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("s/x", Style::default().fg(Gruvbox::FG)),
        Span::styled(" stop  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("X", Style::default().fg(Gruvbox::FG)),
        Span::styled(" cleanup  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("r", Style::default().fg(Gruvbox::FG)),
        Span::styled(" refresh  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("q", Style::default().fg(Gruvbox::FG)),
        Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
    ];
    let legend = vec![
        Span::styled("▶", Style::default().fg(Gruvbox::GREEN)),
        Span::styled("run ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("✔", Style::default().fg(Gruvbox::AQUA)),
        Span::styled("done ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("⏹", Style::default().fg(Gruvbox::YELLOW)),
        Span::styled("stop ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("✘", Style::default().fg(Gruvbox::RED)),
        Span::styled("dead", Style::default().fg(Gruvbox::GRAY)),
    ];
    super::app::render_footer_pub(f, chunks[3], help, legend);
}
