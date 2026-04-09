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

/// An entry in the display list — either a section header or a real watcher
#[derive(Debug)]
enum DisplayRow {
    Header(String),
    Entry(usize), // index into the watchers vec
}

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

/// Build grouped display rows: persistent header + items, then ephemeral header + items
fn build_display_rows(watchers: &[Watcher]) -> Vec<DisplayRow> {
    let persistent: Vec<usize> = watchers.iter().enumerate()
        .filter(|(_, w)| w.persistent)
        .map(|(i, _)| i)
        .collect();
    let ephemeral: Vec<usize> = watchers.iter().enumerate()
        .filter(|(_, w)| !w.persistent)
        .map(|(i, _)| i)
        .collect();

    let mut rows = Vec::new();

    if !persistent.is_empty() {
        rows.push(DisplayRow::Header(format!(
            "🔄 Persistent ({})",
            persistent.len()
        )));
        for i in persistent {
            rows.push(DisplayRow::Entry(i));
        }
    }

    if !ephemeral.is_empty() {
        if !rows.is_empty() {
            // spacer between groups
            rows.push(DisplayRow::Header(String::new()));
        }
        rows.push(DisplayRow::Header(format!(
            "⚡ Ephemeral ({})",
            ephemeral.len()
        )));
        for i in ephemeral {
            rows.push(DisplayRow::Entry(i));
        }
    }

    rows
}

/// Find next selectable (Entry) index in a given direction
fn next_selectable(rows: &[DisplayRow], current: usize, direction: i32) -> usize {
    let len = rows.len();
    if len == 0 { return 0; }
    let mut idx = current as i32;
    loop {
        idx += direction;
        if idx < 0 { idx = len as i32 - 1; }
        if idx >= len as i32 { idx = 0; }
        if matches!(rows[idx as usize], DisplayRow::Entry(_)) {
            return idx as usize;
        }
        if idx as usize == current { return current; }
    }
}

/// Find first selectable index
fn first_selectable(rows: &[DisplayRow]) -> Option<usize> {
    rows.iter().position(|r| matches!(r, DisplayRow::Entry(_)))
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, store: &Store) -> Result<()> {
    let mut watchers = load_watchers(store)?;
    let mut rows = build_display_rows(&watchers);
    let mut list_state = ListState::default();
    list_state.select(first_selectable(&rows));
    let mut auto_refresh = std::time::Instant::now();

    loop {
        terminal.draw(|f| render(f, f.area(), &watchers, &rows, &mut list_state))?;

        // Auto-refresh every 5 seconds
        if auto_refresh.elapsed() >= Duration::from_secs(5) {
            let old_watcher_idx = list_state.selected()
                .and_then(|i| match &rows[i] { DisplayRow::Entry(wi) => Some(*wi), _ => None });
            watchers = load_watchers(store)?;
            rows = build_display_rows(&watchers);
            // Try to keep same watcher selected
            if let Some(wi) = old_watcher_idx {
                let new_pos = rows.iter().position(|r| matches!(r, DisplayRow::Entry(j) if *j == wi));
                list_state.select(new_pos.or_else(|| first_selectable(&rows)));
            } else {
                list_state.select(first_selectable(&rows));
            }
            auto_refresh = std::time::Instant::now();
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => break,
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(i) = list_state.selected() {
                            list_state.select(Some(next_selectable(&rows, i, -1)));
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(i) = list_state.selected() {
                            list_state.select(Some(next_selectable(&rows, i, 1)));
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Char('s') => {
                        if let Some(w) = selected_watcher(&rows, &watchers, &list_state) {
                            let _ = crate::watcher::stop_watcher(&w.id);
                            refresh(&mut watchers, &mut rows, &mut list_state, store);
                        }
                    }
                    KeyCode::Char('X') => {
                        let _ = crate::watcher::cleanup_watchers();
                        refresh(&mut watchers, &mut rows, &mut list_state, store);
                    }
                    KeyCode::Char('d') => {
                        if let Some(w) = selected_watcher(&rows, &watchers, &list_state) {
                            if w.pid.map(|p| is_process_alive(p)).unwrap_or(false) {
                                let _ = crate::watcher::stop_watcher(&w.id);
                            }
                            let _ = store.delete_watcher(&w.id);
                            refresh(&mut watchers, &mut rows, &mut list_state, store);
                        }
                    }
                    KeyCode::Char('R') => {
                        if let Some(w) = selected_watcher(&rows, &watchers, &list_state) {
                            let _ = restart_watcher(w);
                            std::thread::sleep(Duration::from_millis(500));
                            refresh(&mut watchers, &mut rows, &mut list_state, store);
                        }
                    }
                    KeyCode::Char('r') => {
                        refresh(&mut watchers, &mut rows, &mut list_state, store);
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        if let Some(i) = list_state.selected() {
                            list_state.select(Some(next_selectable(&rows, i, -1)));
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if let Some(i) = list_state.selected() {
                            list_state.select(Some(next_selectable(&rows, i, 1)));
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

fn selected_watcher<'a>(rows: &[DisplayRow], watchers: &'a [Watcher], list_state: &ListState) -> Option<&'a Watcher> {
    list_state.selected()
        .and_then(|i| match &rows[i] {
            DisplayRow::Entry(wi) => watchers.get(*wi),
            _ => None,
        })
}

fn refresh(watchers: &mut Vec<Watcher>, rows: &mut Vec<DisplayRow>, list_state: &mut ListState, store: &Store) {
    *watchers = load_watchers(store).unwrap_or_default();
    *rows = build_display_rows(watchers);
    if let Some(sel) = first_selectable(rows) {
        let current = list_state.selected().unwrap_or(0);
        if current >= rows.len() || !matches!(rows[current], DisplayRow::Entry(_)) {
            list_state.select(Some(sel));
        }
    } else {
        list_state.select(None);
    }
}

/// Restart a watcher by re-spawning with its stored args
fn restart_watcher(w: &Watcher) -> Result<()> {
    let args_str = w.restart_args.as_deref()
        .ok_or_else(|| anyhow::anyhow!("No restart args stored for watcher '{}'", w.id))?;

    let args: Vec<&str> = args_str.split('\x00').collect();
    if args.is_empty() {
        anyhow::bail!("Empty restart args for watcher '{}'", w.id);
    }

    let store = Store::open()?;
    if w.pid.map(|p| is_process_alive(p)).unwrap_or(false) {
        crate::watcher::stop_watcher(&w.id)?;
    }
    store.delete_watcher(&w.id)?;

    let exe = std::env::current_exe()?;
    std::process::Command::new(&exe)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

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
    rows: &[DisplayRow],
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
                "  No watchers running. Use 'pilot watch <type> --id <id>' to start one.",
                Style::default().fg(Gruvbox::GRAY),
            ))),
            chunks[2],
        );
    } else {
        let items: Vec<ListItem> = rows
            .iter()
            .map(|row| match row {
                DisplayRow::Header(title) => {
                    if title.is_empty() {
                        // Spacer row
                        ListItem::new(Line::from(""))
                    } else {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("  {}", title),
                                Style::default()
                                    .fg(Gruvbox::FG_BRIGHT)
                                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                            ),
                        ]))
                    }
                }
                DisplayRow::Entry(idx) => {
                    let w = &watchers[*idx];
                    let (status_text, status_color) = status_display(w);
                    let (icon, icon_color) = type_icon(&w.watcher_type);

                    let time = &w.started_at[..16.min(w.started_at.len())];

                    let mut spans = vec![
                        Span::styled(format!("    {} ", icon), Style::default().fg(icon_color)),
                        Span::styled(
                            format!("{:<20}", w.id),
                            Style::default().fg(Gruvbox::FG_BRIGHT).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{:<12}", status_text),
                            Style::default().fg(status_color),
                        ),
                    ];

                    if let Some(ref output) = w.last_output {
                        if !output.is_empty() {
                            spans.push(Span::styled(
                                format!("  {}  ", output),
                                Style::default().fg(Gruvbox::AQUA),
                            ));
                        }
                    }

                    spans.push(Span::styled(
                        format!("  {}  ", w.watcher_type),
                        Style::default().fg(Gruvbox::DARK_GRAY),
                    ));
                    spans.push(Span::styled(time, Style::default().fg(Gruvbox::DARK_GRAY)));

                    ListItem::new(Line::from(spans))
                }
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
    let can_restart = selected_watcher(rows, watchers, list_state)
        .map(|w| w.restart_args.is_some() && !is_watcher_alive(w))
        .unwrap_or(false);

    let mut help = vec![
        Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
        Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("s", Style::default().fg(Gruvbox::FG)),
        Span::styled(" stop  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("d", Style::default().fg(Gruvbox::FG)),
        Span::styled(" delete  ", Style::default().fg(Gruvbox::GRAY)),
    ];
    if can_restart {
        help.push(Span::styled("R", Style::default().fg(Gruvbox::GREEN)));
        help.push(Span::styled(" restart  ", Style::default().fg(Gruvbox::GRAY)));
    }
    help.extend([
        Span::styled("X", Style::default().fg(Gruvbox::FG)),
        Span::styled(" cleanup  ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("q", Style::default().fg(Gruvbox::FG)),
        Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
    ]);

    let legend = vec![
        Span::styled("🔄", Style::default()),
        Span::styled("persist ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("⚡", Style::default()),
        Span::styled("ephemeral ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("▶", Style::default().fg(Gruvbox::GREEN)),
        Span::styled("run ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("✔", Style::default().fg(Gruvbox::AQUA)),
        Span::styled("done ", Style::default().fg(Gruvbox::GRAY)),
        Span::styled("⏹", Style::default().fg(Gruvbox::YELLOW)),
        Span::styled("stop ", Style::default().fg(Gruvbox::GRAY)),
    ];
    super::app::render_footer_pub(f, chunks[3], help, legend);
}

fn is_watcher_alive(w: &Watcher) -> bool {
    w.pid.map(|p| is_process_alive(p)).unwrap_or(false)
}
