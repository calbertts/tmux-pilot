use anyhow::{bail, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;

use crate::azdo;
use crate::config::{AppConfig, AzdoConfig, AzdoFilters};
use crate::tui::theme::Gruvbox;

/// Run the setup wizard. Returns true if config was saved.
pub async fn run_wizard(cfg: &mut AppConfig) -> Result<bool> {
    // Check if PAT is available
    std::env::var("AZURE_DEVOPS_PAT")
        .or_else(|_| std::env::var("TCS_AZDO_PAT"))
        .context("No AzDo PAT found. Set AZURE_DEVOPS_PAT or TCS_AZDO_PAT first.")?;

    println!("🔧 tcs setup wizard\n");

    // Step 1: Organization
    let org = text_prompt("AzDo organization", Some("nn-bank"))?;

    // Step 2: Fetch projects and let user pick
    println!("\n  Fetching projects from {}...", org);
    let projects = azdo::fetch_projects(&org)?;
    if projects.is_empty() {
        bail!("No projects found in organization '{}'", org);
    }
    let project = pick_from_list("Select project", &projects)?;

    // Step 3: Fetch teams and let user pick
    println!("\n  Fetching teams from {}/{}...", org, project);
    let teams = azdo::fetch_teams(&org, &project)?;
    let team = if teams.is_empty() {
        println!("  No teams found, skipping.");
        None
    } else {
        Some(pick_from_list("Select team", &teams)?)
    };

    // Step 4: Fetch area paths
    println!("\n  Fetching area paths...");
    let areas = azdo::fetch_area_paths(&org, &project)?;
    let area_path = if areas.is_empty() {
        println!("  No area paths found, skipping.");
        None
    } else {
        Some(pick_from_list("Select area path", &areas)?)
    };

    // Step 5: Iteration filter
    let iteration = text_prompt(
        "Iteration filter (\"current\", specific path, or empty for all)",
        Some(""),
    )?;

    // Save
    let azdo = AzdoConfig {
        organization: org,
        project,
        team,
        filters: AzdoFilters {
            iteration,
            states: vec![
                "New".to_string(),
                "Active".to_string(),
                "Resolved".to_string(),
            ],
            area_paths: area_path.map(|a| vec![a]).unwrap_or_default(),
        },
    };

    cfg.azdo = Some(azdo);
    cfg.save()?;

    println!("\n✅ Config saved to {}", crate::config::config_path().display());
    println!("   Run `tcs config` to review, or `tcs open` to start.");

    Ok(true)
}

/// Simple text prompt with optional default
fn text_prompt(label: &str, default: Option<&str>) -> Result<String> {
    let default_display = default.unwrap_or("");
    if default_display.is_empty() {
        eprint!("  {} > ", label);
    } else {
        eprint!("  {} [{}] > ", label, default_display);
    }

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();

    if input.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(input)
    }
}

/// TUI list picker — shows a fuzzy-filterable list and returns the selected item
fn pick_from_list(title: &str, items: &[String]) -> Result<String> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = ListState::default();
    state.select(Some(0));
    let mut filter = String::new();
    let mut filtered: Vec<usize> = (0..items.len()).collect();

    let result = loop {
        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(Clear, area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

            let search_text = if filter.is_empty() {
                "🔍 Type to filter...".to_string()
            } else {
                format!("🔍 {}", filter)
            };
            let search = Paragraph::new(search_text)
                .style(Style::default().fg(Gruvbox::FG_BRIGHT).bg(Gruvbox::BG))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Gruvbox::GREEN))
                        .title(format!(" {} ", title))
                        .title_style(
                            Style::default()
                                .fg(Gruvbox::ORANGE)
                                .add_modifier(Modifier::BOLD),
                        ),
                );
            f.render_widget(search, chunks[0]);

            let list_items: Vec<ListItem> = filtered
                .iter()
                .map(|&idx| {
                    ListItem::new(Line::from(Span::styled(
                        &items[idx],
                        Style::default().fg(Gruvbox::FG_BRIGHT),
                    )))
                })
                .collect();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Gruvbox::DARK_GRAY)),
                )
                .highlight_style(
                    Style::default()
                        .bg(Gruvbox::DARK_GRAY)
                        .fg(Gruvbox::GREEN)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            f.render_stateful_widget(list, chunks[1], &mut state);

            let help = Paragraph::new(Line::from(vec![
                Span::styled(" j/k", Style::default().fg(Gruvbox::GREEN)),
                Span::styled(" navigate  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("enter", Style::default().fg(Gruvbox::GREEN)),
                Span::styled(" select  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("esc", Style::default().fg(Gruvbox::GREEN)),
                Span::styled(" cancel", Style::default().fg(Gruvbox::GRAY)),
            ]))
            .style(Style::default().bg(Gruvbox::BG));
            f.render_widget(help, chunks[2]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => break Err(anyhow::anyhow!("Cancelled")),
                        KeyCode::Enter => {
                            if let Some(sel) = state.selected() {
                                if let Some(&idx) = filtered.get(sel) {
                                    break Ok(items[idx].clone());
                                }
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if !filtered.is_empty() {
                                let i = state.selected().unwrap_or(0);
                                state.select(Some(if i == 0 {
                                    filtered.len() - 1
                                } else {
                                    i - 1
                                }));
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !filtered.is_empty() {
                                let i = state.selected().unwrap_or(0);
                                state.select(Some((i + 1) % filtered.len()));
                            }
                        }
                        KeyCode::Backspace => {
                            filter.pop();
                            update_filter(&filter, items, &mut filtered);
                            state.select(if filtered.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                        }
                        KeyCode::Char(c) if !c.is_ascii_control() => {
                            filter.push(c);
                            update_filter(&filter, items, &mut filtered);
                            state.select(if filtered.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn update_filter(query: &str, items: &[String], filtered: &mut Vec<usize>) {
    if query.is_empty() {
        *filtered = (0..items.len()).collect();
    } else {
        let q = query.to_lowercase();
        *filtered = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
    }
}
