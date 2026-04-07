use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
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
use crate::store::{Notification, Store};

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
    let mut notifications = store.list_notifications(200)?;
    let mut list_state = ListState::default();
    if !notifications.is_empty() {
        list_state.select(Some(0));
    }

    loop {
        terminal.draw(|f| render(f, f.area(), &notifications, &mut list_state))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => break,
                    KeyCode::Up | KeyCode::Char('k') => {
                        let len = notifications.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let len = notifications.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some((i + 1) % len));
                        }
                    }
                    KeyCode::Char('d') => {
                        // Mark selected as read
                        if let Some(i) = list_state.selected() {
                            if i < notifications.len() {
                                store.mark_notification_read(notifications[i].id)?;
                                notifications[i].read = true;
                            }
                        }
                    }
                    KeyCode::Char('D') => {
                        // Mark all as read
                        store.mark_all_read()?;
                        for n in &mut notifications {
                            n.read = true;
                        }
                    }
                    KeyCode::Char('x') => {
                        // Delete selected
                        if let Some(i) = list_state.selected() {
                            if i < notifications.len() {
                                store.delete_notification(notifications[i].id)?;
                                notifications.remove(i);
                                if i >= notifications.len() && !notifications.is_empty() {
                                    list_state.select(Some(notifications.len() - 1));
                                }
                            }
                        }
                    }
                    KeyCode::Enter => {
                        // Open link if available
                        if let Some(i) = list_state.selected() {
                            if let Some(ref link) = notifications[i].link {
                                if link.starts_with("http") {
                                    let _ = std::process::Command::new("open")
                                        .arg(link)
                                        .output();
                                }
                                store.mark_notification_read(notifications[i].id)?;
                                notifications[i].read = true;
                            }
                        }
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        let len = notifications.len();
                        if len > 0 {
                            let i = list_state.selected().unwrap_or(0);
                            list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        let len = notifications.len();
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

    // Refresh tmux status bar on exit
    let _ = std::process::Command::new("tmux")
        .args(["refresh-client", "-S"])
        .output();

    Ok(())
}

fn render(
    f: &mut Frame,
    area: Rect,
    notifications: &[Notification],
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
    let unread = notifications.iter().filter(|n| !n.read).count();
    let title_text = if unread > 0 {
        format!("🔔 Notifications  ({} unread)", unread)
    } else {
        "🔔 Notifications".to_string()
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
    if notifications.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No notifications",
                Style::default().fg(Gruvbox::GRAY),
            ))),
            chunks[2],
        );
    } else {
        let items: Vec<ListItem> = notifications
            .iter()
            .map(|n| {
                let (level_icon, level_color) = match n.level.as_str() {
                    "success" => ("✔", Gruvbox::GREEN),
                    "warn" => ("⚠", Gruvbox::YELLOW),
                    "error" => ("✘", Gruvbox::RED),
                    _ => ("ℹ", Gruvbox::BLUE),
                };
                let read_style = if n.read {
                    Gruvbox::GRAY
                } else {
                    Gruvbox::FG_BRIGHT
                };
                let source_tag = n
                    .source
                    .as_ref()
                    .map(|s| format!("[{}] ", s))
                    .unwrap_or_default();

                let mut spans = vec![
                    Span::styled(
                        if n.read { "  " } else { "• " },
                        Style::default().fg(Gruvbox::ORANGE),
                    ),
                    Span::styled(format!("{} ", level_icon), Style::default().fg(level_color)),
                    Span::styled(source_tag, Style::default().fg(Gruvbox::AQUA)),
                    Span::styled(&n.title, Style::default().fg(read_style)),
                ];

                // Time ago
                spans.push(Span::styled(
                    format!("  {}", &n.created_at[..16.min(n.created_at.len())]),
                    Style::default().fg(Gruvbox::DARK_GRAY),
                ));

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
        Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
        Span::styled(" open link  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("d", Style::default().fg(Gruvbox::FG)),
        Span::styled(" read  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("D", Style::default().fg(Gruvbox::FG)),
        Span::styled(" read all  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("x", Style::default().fg(Gruvbox::FG)),
        Span::styled(" delete  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("q", Style::default().fg(Gruvbox::FG)),
        Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
    ];
    let legend = vec![
        Span::styled("✔", Style::default().fg(Gruvbox::GREEN)),
        Span::styled("ok ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("ℹ", Style::default().fg(Gruvbox::BLUE)),
        Span::styled("info ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("⚠", Style::default().fg(Gruvbox::YELLOW)),
        Span::styled("warn ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("✘", Style::default().fg(Gruvbox::RED)),
        Span::styled("error", Style::default().fg(Gruvbox::GRAY)),
    ];
    super::app::render_footer_pub(f, chunks[3], help, legend);
}
