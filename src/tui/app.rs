use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, MouseButton},
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
use tokio::sync::oneshot;

use super::{fuzzy, theme::Gruvbox};
use crate::{
    azdo,
    config::{AppConfig, AzdoConfig, WorkItem, WorkItemType},
    copilot,
    store::{SessionMapping, Store, WindowMapping},
    tmux,
};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    FeatureSelector,
    TaskSelector,
    Dashboard,
}

#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    Normal,
    /// Text input for creating free sessions or windows
    TextInput,
}

pub struct App<'a> {
    cfg: &'a AppConfig,
    store: Store,
    view: View,
    running: bool,
    input_mode: InputMode,

    // Feature selector state
    features: Vec<FeatureEntry>,
    feature_list_state: ListState,
    feature_filter: String,
    filtered_indices: Vec<usize>,
    /// Maps visual list row → feature index (None = section header)
    visual_map: Vec<Option<usize>>,

    // Task selector state
    tasks: Vec<TaskEntry>,
    task_list_state: ListState,
    task_filter: String,
    task_filtered_indices: Vec<usize>,
    /// Maps visual task row → task index (None = section header)
    task_visual_map: Vec<Option<usize>>,
    current_session: Option<String>,

    // Dashboard state
    dashboard_sessions: Vec<DashboardEntry>,
    dashboard_list_state: ListState,

    // Text input state (for free session/window creation)
    text_input: String,
    text_input_label: String,
    // What action to take when text input is confirmed
    text_input_action: TextInputAction,

    // Status message
    status_msg: Option<String>,

    // Layout area of the main list (for mouse click mapping)
    list_area: Rect,

    // Async loading state
    loading: bool,
    spinner_tick: usize,
    azdo_rx: Option<oneshot::Receiver<AzdoFetchResult>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum FeatureGroup {
    /// Session linked to an AzDo feature
    Linked,
    /// AzDo feature not yet created as a session
    AzdoOnly,
    /// Free session with no AzDo link
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TaskGroup {
    /// Window linked to a Bug
    Bug,
    /// Window linked to a User Story
    UserStory,
    /// Window linked to a Task work item
    Task,
    /// Free window with no AzDo link
    Free,
}

#[derive(Debug, Clone)]
struct FeatureEntry {
    name: String,
    group: FeatureGroup,
    session_exists: bool,
    window_count: usize,
    work_item: Option<WorkItem>,
}

#[derive(Debug, Clone)]
struct TaskEntry {
    name: String,
    group: TaskGroup,
    window_exists: bool,
    window_index: Option<usize>,
    work_item: Option<WorkItem>,
    copilot_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct DashboardEntry {
    session_name: String,
    window_count: usize,
    attached: bool,
    windows: Vec<String>,
    work_item_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
enum TextInputAction {
    None,
    CreateFreeSession,
    CreateNamedWindow,
    CreateCopilotWindow,
}

/// Result from background AzDo fetch
enum AzdoFetchResult {
    Features(Result<Vec<WorkItem>, String>),
    Tasks(Result<Vec<WorkItem>, String>),
}

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl<'a> App<'a> {
    pub fn new(cfg: &'a AppConfig, store: Store, view: View) -> Self {
        Self {
            cfg,
            store,
            view,
            running: true,
            input_mode: InputMode::Normal,
            features: vec![],
            feature_list_state: ListState::default(),
            feature_filter: String::new(),
            filtered_indices: vec![],
            visual_map: vec![],
            tasks: vec![],
            task_list_state: ListState::default(),
            task_filter: String::new(),
            task_filtered_indices: vec![],
            task_visual_map: vec![],
            current_session: None,
            dashboard_sessions: vec![],
            dashboard_list_state: ListState::default(),
            text_input: String::new(),
            text_input_label: String::new(),
            text_input_action: TextInputAction::None,
            status_msg: None,
            list_area: Rect::default(),
            loading: false,
            spinner_tick: 0,
            azdo_rx: None,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Phase 1: load local data (instant: tmux sessions + SQLite)
        self.load_local_data()?;

        // Phase 2: start AzDo fetch in background
        self.start_azdo_fetch();

        let result = self.event_loop(&mut terminal).await;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    /// Load only local data (tmux + SQLite). Instant, no network.
    fn load_local_data(&mut self) -> Result<()> {
        match self.view {
            View::FeatureSelector => self.load_local_features()?,
            View::TaskSelector => self.load_local_tasks()?,
            View::Dashboard => self.load_dashboard()?,
        }
        Ok(())
    }

    /// Spawn background AzDo fetch if configured.
    fn start_azdo_fetch(&mut self) {
        let azdo_cfg = match self.cfg.azdo.clone() {
            Some(cfg) if !cfg.organization.is_empty() && !cfg.project.is_empty() => cfg,
            _ => return,
        };

        match self.view {
            View::FeatureSelector => {
                // Check cache first (instant, on main thread)
                let cache_key = format!("features:{}:{}", azdo_cfg.organization, azdo_cfg.project);
                if let Some(cached) = self.store.get_cached(&cache_key, 15).ok().flatten() {
                    if let Ok(items) = serde_json::from_str::<Vec<WorkItem>>(&cached) {
                        self.merge_azdo_result(AzdoFetchResult::Features(Ok(items)));
                        return;
                    }
                }

                let (tx, rx) = oneshot::channel();
                self.azdo_rx = Some(rx);
                self.loading = true;
                self.status_msg = Some("Loading features from AzDo...".to_string());
                let cfg = azdo_cfg.clone();
                tokio::task::spawn_blocking(move || {
                    let result = Self::fetch_azdo_features_blocking(cfg);
                    tx.send(AzdoFetchResult::Features(result)).ok();
                });
            }
            View::TaskSelector => {
                let session_name = self.current_session.clone().unwrap_or_default();
                let parent_id = self
                    .store
                    .get_session_mapping(&session_name)
                    .ok()
                    .flatten()
                    .and_then(|m| m.work_item_id);

                if let Some(pid) = parent_id {
                    // Check cache first
                    let cache_key = format!(
                        "tasks:{}:{}:{}",
                        azdo_cfg.organization, azdo_cfg.project, pid
                    );
                    if let Some(cached) = self.store.get_cached(&cache_key, 15).ok().flatten() {
                        if let Ok(items) = serde_json::from_str::<Vec<WorkItem>>(&cached) {
                            self.merge_azdo_result(AzdoFetchResult::Tasks(Ok(items)));
                            return;
                        }
                    }

                    let (tx, rx) = oneshot::channel();
                    self.azdo_rx = Some(rx);
                    self.loading = true;
                    self.status_msg = Some("Loading tasks from AzDo...".to_string());
                    let cfg = azdo_cfg.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = Self::fetch_azdo_tasks_blocking(cfg, pid);
                        tx.send(AzdoFetchResult::Tasks(result)).ok();
                    });
                }
            }
            View::Dashboard => {}
        }
    }

    /// Background-safe AzDo feature fetch (no Store, pure HTTP via curl)
    fn fetch_azdo_features_blocking(azdo_cfg: AzdoConfig) -> Result<Vec<WorkItem>, String> {
        azdo::fetch_features_no_cache(&azdo_cfg).map_err(|e| format!("{:#}", e))
    }

    /// Background-safe AzDo task fetch (no Store, pure HTTP via curl)
    fn fetch_azdo_tasks_blocking(azdo_cfg: AzdoConfig, parent_id: u64) -> Result<Vec<WorkItem>, String> {
        azdo::fetch_tasks_no_cache(&azdo_cfg, parent_id).map_err(|e| format!("{:#}", e))
    }

    /// Merge AzDo results into current feature/task lists
    fn merge_azdo_result(&mut self, result: AzdoFetchResult) {
        self.loading = false;
        match result {
            AzdoFetchResult::Features(Ok(ref azdo_features)) => {
                // Cache for next time
                if let Some(ref azdo_cfg) = self.cfg.azdo {
                    let cache_key =
                        format!("features:{}:{}", azdo_cfg.organization, azdo_cfg.project);
                    if let Ok(json) = serde_json::to_string(azdo_features) {
                        self.store.set_cached(&cache_key, &json).ok();
                    }
                }

                let existing_ids: Vec<Option<u64>> = self
                    .features
                    .iter()
                    .filter_map(|f| f.work_item.as_ref().map(|wi| wi.id))
                    .collect();

                for wi in azdo_features {
                    if !existing_ids.contains(&wi.id) {
                        self.features.push(FeatureEntry {
                            name: wi
                                .id
                                .map(|id| format!("#{} {}", id, wi.title))
                                .unwrap_or_else(|| wi.title.clone()),
                            group: FeatureGroup::AzdoOnly,
                            session_exists: false,
                            window_count: 0,
                            work_item: Some(wi.clone()),
                        });
                    }
                }
                self.status_msg = None;
                self.sort_features();
                self.update_feature_filter();
                if self.feature_list_state.selected().is_none() {
                    self.feature_list_state
                        .select(self.first_selectable_row());
                }
            }
            AzdoFetchResult::Features(Err(e)) => {
                self.status_msg = Some(format!("⚠ AzDo: {}", e));
            }
            AzdoFetchResult::Tasks(Ok(ref azdo_tasks)) => {
                // Cache for next time
                if let Some(ref azdo_cfg) = self.cfg.azdo {
                    if let Some(ref session) = self.current_session {
                        if let Some(pid) = self
                            .store
                            .get_session_mapping(session)
                            .ok()
                            .flatten()
                            .and_then(|m| m.work_item_id)
                        {
                            let cache_key = format!(
                                "tasks:{}:{}:{}",
                                azdo_cfg.organization, azdo_cfg.project, pid
                            );
                            if let Ok(json) = serde_json::to_string(azdo_tasks) {
                                self.store.set_cached(&cache_key, &json).ok();
                            }
                        }
                    }
                }

                let existing_ids: Vec<Option<u64>> = self
                    .tasks
                    .iter()
                    .filter_map(|t| t.work_item.as_ref().map(|wi| wi.id))
                    .collect();

                for wi in azdo_tasks {
                    if !existing_ids.contains(&wi.id) {
                        let group = match wi.work_item_type {
                            WorkItemType::Bug => TaskGroup::Bug,
                            WorkItemType::UserStory => TaskGroup::UserStory,
                            WorkItemType::Task => TaskGroup::Task,
                            _ => TaskGroup::Free,
                        };
                        self.tasks.push(TaskEntry {
                            name: wi.display_label(),
                            group,
                            window_exists: false,
                            window_index: None,
                            work_item: Some(wi.clone()),
                            copilot_session_id: None,
                        });
                    }
                }
                self.status_msg = None;
                self.sort_tasks();
                self.update_task_filter();
                if self.task_list_state.selected().is_none() {
                    self.task_list_state
                        .select(self.first_selectable_task_row());
                }
            }
            AzdoFetchResult::Tasks(Err(e)) => {
                self.status_msg = Some(format!("⚠ AzDo: {}", e));
            }
        }
    }

    fn load_local_features(&mut self) -> Result<()> {
        let sessions = tmux::list_sessions().unwrap_or_default();

        self.features = sessions
            .iter()
            .map(|s| {
                let mapping = self.store.get_session_mapping(&s.name).ok().flatten();
                let has_work_item = mapping
                    .as_ref()
                    .map(|m| m.work_item_id.is_some())
                    .unwrap_or(false);
                let group = if has_work_item {
                    FeatureGroup::Linked
                } else {
                    FeatureGroup::Free
                };
                FeatureEntry {
                    name: s.name.clone(),
                    group,
                    session_exists: true,
                    window_count: s.window_count,
                    work_item: mapping.and_then(|m| {
                        Some(WorkItem {
                            id: m.work_item_id,
                            title: m.work_item_title.unwrap_or_default(),
                            work_item_type: match m.work_item_type.as_deref() {
                                Some("Feature") => WorkItemType::Feature,
                                Some("User Story") => WorkItemType::UserStory,
                                Some("Bug") => WorkItemType::Bug,
                                Some("Task") => WorkItemType::Task,
                                _ => WorkItemType::Free,
                            },
                            state: String::new(),
                            assigned_to: None,
                            description: None,
                            acceptance_criteria: None,
                            parent_id: None,
                        })
                    }),
                }
            })
            .collect();

        self.sort_features();
        self.update_feature_filter();
        self.feature_list_state
            .select(self.first_selectable_row());
        Ok(())
    }

    /// Sort features: Linked first, then AzDo-only, then Free
    fn sort_features(&mut self) {
        self.features.sort_by(|a, b| {
            a.group.cmp(&b.group).then_with(|| a.name.cmp(&b.name))
        });
    }

    /// Find the first row in visual_map that is a selectable item (not a header/spacer)
    fn first_selectable_row(&self) -> Option<usize> {
        self.visual_map
            .iter()
            .position(|entry| entry.is_some())
    }

    /// Sort tasks: Bugs first, then User Stories, then Tasks, then Free
    fn sort_tasks(&mut self) {
        self.tasks.sort_by(|a, b| {
            a.group.cmp(&b.group).then_with(|| a.name.cmp(&b.name))
        });
    }

    /// Find the first selectable task row
    fn first_selectable_task_row(&self) -> Option<usize> {
        self.task_visual_map
            .iter()
            .position(|entry| entry.is_some())
    }

    fn load_local_tasks(&mut self) -> Result<()> {
        // Use pre-set session (from feature selector 'o') or fall back to tmux current
        let session_name = self
            .current_session
            .clone()
            .unwrap_or_else(|| tmux::current_session_name().unwrap_or_default());
        self.current_session = Some(session_name.clone());

        let windows = tmux::list_windows(&session_name).unwrap_or_default();
        let window_mappings = self
            .store
            .get_window_mappings(&session_name)
            .unwrap_or_default();

        self.tasks = windows
            .iter()
            .map(|w| {
                let mapping = window_mappings.iter().find(|m| m.window_name == w.name);
                let work_item = mapping.and_then(|m| {
                    m.work_item_id.map(|id| WorkItem {
                        id: Some(id),
                        title: m.work_item_title.clone().unwrap_or_default(),
                        work_item_type: match m.work_item_type.as_deref() {
                            Some("User Story") => WorkItemType::UserStory,
                            Some("Bug") => WorkItemType::Bug,
                            Some("Task") => WorkItemType::Task,
                            _ => WorkItemType::Free,
                        },
                        state: String::new(),
                        assigned_to: None,
                        description: None,
                        acceptance_criteria: None,
                        parent_id: None,
                    })
                });
                let group = match work_item.as_ref().map(|wi| &wi.work_item_type) {
                    Some(WorkItemType::Bug) => TaskGroup::Bug,
                    Some(WorkItemType::UserStory) => TaskGroup::UserStory,
                    Some(WorkItemType::Task) => TaskGroup::Task,
                    _ => TaskGroup::Free,
                };
                TaskEntry {
                    name: w.name.clone(),
                    group,
                    window_exists: true,
                    window_index: Some(w.index),
                    work_item,
                    copilot_session_id: mapping.and_then(|m| m.copilot_session_id.clone()),
                }
            })
            .collect();

        self.sort_tasks();
        self.update_task_filter();
        self.task_list_state
            .select(self.first_selectable_task_row());
        Ok(())
    }

    fn load_dashboard(&mut self) -> Result<()> {
        let sessions = tmux::list_sessions().unwrap_or_default();

        self.dashboard_sessions = sessions
            .into_iter()
            .map(|s| {
                let mapping = self.store.get_session_mapping(&s.name).ok().flatten();
                let windows = tmux::list_windows(&s.name)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|w| w.name)
                    .collect();
                DashboardEntry {
                    session_name: s.name,
                    window_count: s.window_count,
                    attached: s.attached,
                    windows,
                    work_item_id: mapping.and_then(|m| m.work_item_id),
                }
            })
            .collect();

        if !self.dashboard_sessions.is_empty() {
            self.dashboard_list_state.select(Some(0));
        }
        Ok(())
    }

    fn update_feature_filter(&mut self) {
        let labels: Vec<String> = self.features.iter().map(|f| f.name.clone()).collect();
        let results = fuzzy::fuzzy_match(&self.feature_filter, &labels);
        self.filtered_indices = results.into_iter().map(|(idx, _)| idx).collect();

        // Build visual_map with section headers
        self.visual_map.clear();
        let mut prev_group: Option<FeatureGroup> = None;
        for &idx in &self.filtered_indices {
            let group = &self.features[idx].group;
            if prev_group.as_ref() != Some(group) {
                if prev_group.is_some() {
                    self.visual_map.push(None); // spacer
                }
                self.visual_map.push(None); // section header
                prev_group = Some(group.clone());
            }
            self.visual_map.push(Some(idx));
        }
    }

    fn update_task_filter(&mut self) {
        let labels: Vec<String> = self.tasks.iter().map(|t| t.name.clone()).collect();
        let results = fuzzy::fuzzy_match(&self.task_filter, &labels);
        self.task_filtered_indices = results.into_iter().map(|(idx, _)| idx).collect();

        // Build task_visual_map with section headers
        self.task_visual_map.clear();
        let mut prev_group: Option<TaskGroup> = None;
        for &idx in &self.task_filtered_indices {
            let group = &self.tasks[idx].group;
            if prev_group.as_ref() != Some(group) {
                if prev_group.is_some() {
                    self.task_visual_map.push(None); // spacer
                }
                self.task_visual_map.push(None); // section header
                prev_group = Some(group.clone());
            }
            self.task_visual_map.push(Some(idx));
        }
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        while self.running {
            // Check for background AzDo result
            if let Some(ref mut rx) = self.azdo_rx {
                if let Ok(result) = rx.try_recv() {
                    let result = result;
                    self.azdo_rx = None;
                    self.merge_azdo_result(result);
                }
            }

            if self.loading {
                self.spinner_tick = self.spinner_tick.wrapping_add(1);
            }

            terminal.draw(|f| self.render(f))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            self.handle_key(key)?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        // Handle text input mode
        if self.input_mode == InputMode::TextInput {
            return self.handle_text_input_key(key.code);
        }

        // Ctrl+O: toggle between Feature ↔ Task selector from any view
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
            return self.switch_to_view(match self.view {
                View::FeatureSelector => View::TaskSelector,
                View::TaskSelector => View::FeatureSelector,
                View::Dashboard => View::FeatureSelector,
            });
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.running = false;
            }
            _ => match self.view {
                View::FeatureSelector => self.handle_feature_key(key.code)?,
                View::TaskSelector => self.handle_task_key(key.code)?,
                View::Dashboard => self.handle_dashboard_key(key.code)?,
            },
        }
        Ok(())
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Result<()> {
        if self.input_mode == InputMode::TextInput {
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.move_selection_up(&self.view.clone());
            }
            MouseEventKind::ScrollDown => {
                self.move_selection_down(&self.view.clone());
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let row = mouse.row;
                let list_offset = self.list_area.y;
                if row >= list_offset && row < list_offset + self.list_area.height {
                    let clicked_row = (row - list_offset) as usize;
                    // Offset by current scroll position
                    let scroll_offset = self.current_scroll_offset();
                    let target_row = clicked_row + scroll_offset;
                    self.select_row_if_valid(target_row);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // double-click handled by terminal, single click selects
            }
            _ => {}
        }
        Ok(())
    }

    /// Get the current scroll offset from list state
    fn current_scroll_offset(&self) -> usize {
        match self.view {
            View::FeatureSelector => self.feature_list_state.offset(),
            View::TaskSelector => self.task_list_state.offset(),
            View::Dashboard => self.dashboard_list_state.offset(),
        }
    }

    /// Select a row if it's a valid selectable item
    fn select_row_if_valid(&mut self, row: usize) {
        match self.view {
            View::FeatureSelector => {
                if row < self.visual_map.len() && self.visual_map[row].is_some() {
                    self.feature_list_state.select(Some(row));
                }
            }
            View::TaskSelector => {
                if row < self.task_visual_map.len() && self.task_visual_map[row].is_some() {
                    self.task_list_state.select(Some(row));
                }
            }
            View::Dashboard => {
                if row < self.dashboard_sessions.len() {
                    self.dashboard_list_state.select(Some(row));
                }
            }
        }
    }

    /// Switch to a different view, reloading its data
    fn switch_to_view(&mut self, target: View) -> Result<()> {
        self.switch_to_view_with_session(target, None)
    }

    /// Switch to a different view, optionally setting the session context
    fn switch_to_view_with_session(&mut self, target: View, session: Option<String>) -> Result<()> {
        self.view = target;
        self.loading = false;
        self.status_msg = None;
        self.azdo_rx = None;
        if let Some(name) = session {
            self.current_session = Some(name);
        }
        self.load_local_data()?;
        self.start_azdo_fetch();
        Ok(())
    }

    fn handle_text_input_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                let text = self.text_input.trim().to_string();
                if !text.is_empty() {
                    match self.text_input_action.clone() {
                        TextInputAction::CreateFreeSession => {
                            self.do_create_free_session(&text)?;
                        }
                        TextInputAction::CreateNamedWindow => {
                            self.do_create_terminal_window(&text)?;
                        }
                        TextInputAction::CreateCopilotWindow => {
                            self.do_create_copilot_window(Some(&text))?;
                        }
                        TextInputAction::None => {}
                    }
                }
                self.input_mode = InputMode::Normal;
                self.text_input.clear();
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.text_input.clear();
            }
            KeyCode::Backspace => {
                self.text_input.pop();
            }
            KeyCode::Char(c) => {
                self.text_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_feature_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(&View::FeatureSelector),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(&View::FeatureSelector),
            KeyCode::Enter => self.select_feature()?,
            KeyCode::Char('o') => {
                // Get the selected feature's session name
                let session = self.feature_list_state.selected()
                    .and_then(|sel| self.visual_map.get(sel).copied().flatten())
                    .map(|idx| self.features[idx].name.clone());
                self.switch_to_view_with_session(View::TaskSelector, session)?;
            }
            KeyCode::Char('n') => {
                self.input_mode = InputMode::TextInput;
                self.text_input_label = "New session name".to_string();
                self.text_input_action = TextInputAction::CreateFreeSession;
            }
            KeyCode::Backspace => {
                self.feature_filter.pop();
                self.update_feature_filter();
                self.feature_list_state
                    .select(self.first_selectable_row());
            }
            KeyCode::Char(c) if !c.is_ascii_control() => {
                self.feature_filter.push(c);
                self.update_feature_filter();
                self.feature_list_state
                    .select(self.first_selectable_row());
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_task_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(&View::TaskSelector),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(&View::TaskSelector),
            KeyCode::Enter => self.select_task()?,
            KeyCode::Char('o') => {
                self.switch_to_view(View::FeatureSelector)?;
            }
            KeyCode::Char('c') => {
                // Create copilot window directly
                self.do_create_copilot_window(None)?;
            }
            KeyCode::Char('t') => {
                self.input_mode = InputMode::TextInput;
                self.text_input_label = "New terminal window name".to_string();
                self.text_input_action = TextInputAction::CreateNamedWindow;
            }
            KeyCode::Backspace => {
                self.task_filter.pop();
                self.update_task_filter();
                self.task_list_state
                    .select(self.first_selectable_task_row());
            }
            KeyCode::Char(ch) if !ch.is_ascii_control() => {
                self.task_filter.push(ch);
                self.update_task_filter();
                self.task_list_state
                    .select(self.first_selectable_task_row());
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dashboard_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(&View::Dashboard),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(&View::Dashboard),
            KeyCode::Enter => self.attach_dashboard_session()?,
            KeyCode::Char('o') => {
                self.switch_to_view(View::FeatureSelector)?;
            }
            KeyCode::Char('d') => self.kill_dashboard_session()?,
            _ => {}
        }
        Ok(())
    }

    fn move_selection_up(&mut self, view: &View) {
        match view {
            View::FeatureSelector => {
                let len = self.visual_map.len();
                if len == 0 { return; }
                let mut i = self.feature_list_state.selected().unwrap_or(0);
                loop {
                    i = if i == 0 { len - 1 } else { i - 1 };
                    if self.visual_map[i].is_some() { break; }
                }
                self.feature_list_state.select(Some(i));
            }
            View::TaskSelector => {
                let len = self.task_visual_map.len();
                if len == 0 { return; }
                let mut i = self.task_list_state.selected().unwrap_or(0);
                loop {
                    i = if i == 0 { len - 1 } else { i - 1 };
                    if self.task_visual_map[i].is_some() { break; }
                }
                self.task_list_state.select(Some(i));
            }
            View::Dashboard => {
                let len = self.dashboard_sessions.len();
                if len == 0 { return; }
                let i = self.dashboard_list_state.selected().unwrap_or(0);
                self.dashboard_list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
        }
    }

    fn move_selection_down(&mut self, view: &View) {
        match view {
            View::FeatureSelector => {
                let len = self.visual_map.len();
                if len == 0 { return; }
                let mut i = self.feature_list_state.selected().unwrap_or(0);
                loop {
                    i = (i + 1) % len;
                    if self.visual_map[i].is_some() { break; }
                }
                self.feature_list_state.select(Some(i));
            }
            View::TaskSelector => {
                let len = self.task_visual_map.len();
                if len == 0 { return; }
                let mut i = self.task_list_state.selected().unwrap_or(0);
                loop {
                    i = (i + 1) % len;
                    if self.task_visual_map[i].is_some() { break; }
                }
                self.task_list_state.select(Some(i));
            }
            View::Dashboard => {
                let len = self.dashboard_sessions.len();
                if len == 0 { return; }
                let i = self.dashboard_list_state.selected().unwrap_or(0);
                self.dashboard_list_state.select(Some((i + 1) % len));
            }
        }
    }

    // ─── Actions ───────────────────────────────────────────────

    fn select_feature(&mut self) -> Result<()> {
        if let Some(selected) = self.feature_list_state.selected() {
            // visual_map maps visual rows → feature indices (None = header/spacer)
            let idx = match self.visual_map.get(selected).copied().flatten() {
                Some(i) => i,
                None => return Ok(()), // header row, do nothing
            };
            {
                let feature = self.features[idx].clone();
                if feature.session_exists {
                    tmux::switch_session(&feature.name)?;
                } else {
                    // Create new session for this feature
                    tmux::create_session(&feature.name, None)?;

                    // Persist mapping
                    self.store.save_session_mapping(&SessionMapping {
                        session_name: feature.name.clone(),
                        work_item_id: feature.work_item.as_ref().and_then(|wi| wi.id),
                        work_item_title: feature
                            .work_item
                            .as_ref()
                            .map(|wi| wi.title.clone()),
                        work_item_type: feature
                            .work_item
                            .as_ref()
                            .map(|wi| wi.work_item_type.to_string()),
                        template: None,
                        created_at: String::new(),
                    })?;

                    // Launch copilot in first window
                    if self.cfg.copilot.auto_launch {
                        let target = format!("{}:1", feature.name);
                        copilot::launch_in_target(
                            self.cfg,
                            &target,
                            feature.work_item.as_ref(),
                        )?;

                        // Track window mapping
                        self.store.save_window_mapping(&WindowMapping {
                            session_name: feature.name.clone(),
                            window_name: "copilot".to_string(),
                            work_item_id: feature
                                .work_item
                                .as_ref()
                                .and_then(|wi| wi.id),
                            work_item_title: feature
                                .work_item
                                .as_ref()
                                .map(|wi| wi.title.clone()),
                            work_item_type: feature
                                .work_item
                                .as_ref()
                                .map(|wi| wi.work_item_type.to_string()),
                            copilot_session_id: None,
                            window_type: "copilot".to_string(),
                        })?;

                        // Rename window
                        tmux::rename_window(&feature.name, 1, "copilot")?;
                    }

                    tmux::switch_session(&feature.name)?;
                }
                self.running = false;
            }
        }
        Ok(())
    }

    fn select_task(&mut self) -> Result<()> {
        if let Some(selected) = self.task_list_state.selected() {
            let idx = match self.task_visual_map.get(selected).copied().flatten() {
                Some(i) => i,
                None => return Ok(()), // header row
            };
            {
                let task = self.tasks[idx].clone();
                let session = self
                    .current_session
                    .clone()
                    .unwrap_or_else(|| "default".to_string());

                if task.window_exists {
                    if let Some(win_idx) = task.window_index {
                        tmux::select_window(&session, win_idx)?;
                    }
                } else {
                    // Create new window for this AzDo task
                    let win_name = task
                        .work_item
                        .as_ref()
                        .map(|wi| {
                            wi.id
                                .map(|id| format!("#{} {}", id, truncate(&wi.title, 30)))
                                .unwrap_or_else(|| truncate(&wi.title, 30))
                        })
                        .unwrap_or_else(|| task.name.clone());

                    tmux::create_window(&session, &win_name, None)?;

                    // Launch copilot with work item context
                    if self.cfg.copilot.auto_launch {
                        copilot::launch_in_current_pane(self.cfg, task.work_item.as_ref())?;
                    }

                    // Persist window mapping
                    self.store.save_window_mapping(&WindowMapping {
                        session_name: session,
                        window_name: win_name,
                        work_item_id: task.work_item.as_ref().and_then(|wi| wi.id),
                        work_item_title: task
                            .work_item
                            .as_ref()
                            .map(|wi| wi.title.clone()),
                        work_item_type: task
                            .work_item
                            .as_ref()
                            .map(|wi| wi.work_item_type.to_string()),
                        copilot_session_id: None,
                        window_type: "copilot".to_string(),
                    })?;
                }
                self.running = false;
            }
        }
        Ok(())
    }

    fn do_create_free_session(&mut self, name: &str) -> Result<()> {
        if tmux::session_exists(name)? {
            tmux::switch_session(name)?;
        } else {
            tmux::create_session(name, None)?;

            self.store.save_session_mapping(&SessionMapping {
                session_name: name.to_string(),
                work_item_id: None,
                work_item_title: None,
                work_item_type: Some("Free".to_string()),
                template: None,
                created_at: String::new(),
            })?;

            if self.cfg.copilot.auto_launch {
                let target = format!("{}:1", name);
                copilot::launch_in_target(self.cfg, &target, None)?;
                tmux::rename_window(name, 1, "copilot")?;
            }

            tmux::switch_session(name)?;
        }
        self.running = false;
        Ok(())
    }

    fn do_create_copilot_window(&mut self, name: Option<&str>) -> Result<()> {
        if let Some(ref session) = self.current_session.clone() {
            let win_name = name.unwrap_or("copilot");
            tmux::create_window(session, win_name, None)?;
            copilot::launch_in_current_pane(self.cfg, None)?;

            self.store.save_window_mapping(&WindowMapping {
                session_name: session.clone(),
                window_name: win_name.to_string(),
                work_item_id: None,
                work_item_title: None,
                work_item_type: None,
                copilot_session_id: None,
                window_type: "copilot".to_string(),
            })?;
        }
        self.running = false;
        Ok(())
    }

    fn do_create_terminal_window(&mut self, name: &str) -> Result<()> {
        if let Some(ref session) = self.current_session.clone() {
            tmux::create_window(session, name, None)?;

            self.store.save_window_mapping(&WindowMapping {
                session_name: session.clone(),
                window_name: name.to_string(),
                work_item_id: None,
                work_item_title: None,
                work_item_type: None,
                copilot_session_id: None,
                window_type: "shell".to_string(),
            })?;
        }
        self.running = false;
        Ok(())
    }

    fn attach_dashboard_session(&mut self) -> Result<()> {
        if let Some(selected) = self.dashboard_list_state.selected() {
            if let Some(entry) = self.dashboard_sessions.get(selected) {
                tmux::switch_session(&entry.session_name)?;
                self.running = false;
            }
        }
        Ok(())
    }

    fn kill_dashboard_session(&mut self) -> Result<()> {
        if let Some(selected) = self.dashboard_list_state.selected() {
            if let Some(entry) = self.dashboard_sessions.get(selected) {
                let name = entry.session_name.clone();
                tmux::kill_session(&name)?;
                self.store.delete_session_mapping(&name).ok();
                self.load_dashboard()?;
            }
        }
        Ok(())
    }

    // ─── Rendering ─────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        let area = f.area();
        f.render_widget(Clear, area);

        match self.view {
            View::FeatureSelector => self.render_feature_selector(f, area),
            View::TaskSelector => self.render_task_selector(f, area),
            View::Dashboard => self.render_dashboard(f, area),
        }

        // Render text input overlay if active
        if self.input_mode == InputMode::TextInput {
            self.render_text_input(f, area);
        }
    }

    fn render_text_input(&self, f: &mut Frame, area: Rect) {
        let popup_width = 50.min(area.width - 4);
        let popup_height = 3;
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        f.render_widget(Clear, popup_area);

        let input = Paragraph::new(format!("{}_", self.text_input))
            .style(Style::default().fg(Gruvbox::FG_BRIGHT).bg(Gruvbox::BG))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Gruvbox::ORANGE))
                    .title(format!(" {} ", self.text_input_label))
                    .title_style(
                        Style::default()
                            .fg(Gruvbox::ORANGE)
                            .add_modifier(Modifier::BOLD),
                    ),
            );
        f.render_widget(input, popup_area);
    }

    fn render_feature_selector(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // search
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // help/status
            ])
            .split(area);

        // Search line — minimal, no box
        let search_text = if self.feature_filter.is_empty() {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled(
                    "filter...",
                    Style::default().fg(Gruvbox::FG),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(Gruvbox::ORANGE)),
                Span::styled(
                    self.feature_filter.as_str(),
                    Style::default()
                        .fg(Gruvbox::FG_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        };
        f.render_widget(Paragraph::new(search_text), chunks[0]);

        // Build grouped list items with section headers
        let mut items: Vec<ListItem> = Vec::new();
        let mut prev_group: Option<&FeatureGroup> = None;

        for &idx in &self.filtered_indices {
            let feature = &self.features[idx];

            // Insert section header when group changes
            if prev_group != Some(&feature.group) {
                if prev_group.is_some() {
                    items.push(ListItem::new(Line::from(""))); // spacer
                }
                let header = match feature.group {
                    FeatureGroup::Linked => "  ─── Active ───",
                    FeatureGroup::AzdoOnly => "  ─── AzDo ───",
                    FeatureGroup::Free => "  ─── Free ───",
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    header,
                    Style::default()
                        .fg(Gruvbox::GRAY)
                        .add_modifier(Modifier::DIM),
                ))));
                prev_group = Some(&feature.group);
            }

            let (icon, name_color, detail) = match feature.group {
                FeatureGroup::Linked => {
                    let id_str = feature
                        .work_item
                        .as_ref()
                        .and_then(|wi| wi.id.map(|id| format!(" #{}", id)))
                        .unwrap_or_default();
                    (
                        "🏗",
                        Gruvbox::GREEN,
                        format!("{}  {}w", id_str, feature.window_count),
                    )
                }
                FeatureGroup::AzdoOnly => ("🏗", Gruvbox::GRAY, " ⊕ new".to_string()),
                FeatureGroup::Free => (
                    "📂",
                    Gruvbox::FG,
                    format!("{}w", feature.window_count),
                ),
            };

            let detail_color = match feature.group {
                FeatureGroup::AzdoOnly => Gruvbox::GREEN,
                _ => Gruvbox::GRAY,
            };

            items.push(ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{} ", icon), Style::default().fg(name_color)),
                Span::styled(&feature.name, Style::default().fg(name_color)),
                Span::styled(format!("  {}", detail), Style::default().fg(detail_color)),
            ])));
        }

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        self.list_area = chunks[2];
        f.render_stateful_widget(list, chunks[2], &mut self.feature_list_state);

        // Bottom bar: help + status
        let bottom = if let Some(ref msg) = self.status_msg {
            let frame = if self.loading {
                SPINNER[self.spinner_tick % SPINNER.len()]
            } else {
                "⚠"
            };
            Line::from(Span::styled(
                format!("  {} {}", frame, msg),
                Style::default().fg(Gruvbox::YELLOW),
            ))
        } else {
            Line::from(vec![
                Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
                Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
                Span::styled(" open  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("o", Style::default().fg(Gruvbox::FG)),
                Span::styled(" tasks  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("n", Style::default().fg(Gruvbox::FG)),
                Span::styled(" new  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("q", Style::default().fg(Gruvbox::FG)),
                Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
            ])
        };
        f.render_widget(Paragraph::new(bottom), chunks[3]);
    }

    fn render_task_selector(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // session title
                Constraint::Length(1), // search
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // help/status
            ])
            .split(area);

        // Session title line
        let title = self
            .current_session
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    &title,
                    Style::default()
                        .fg(Gruvbox::ORANGE)
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[0],
        );

        // Search line — minimal, no box
        let search_text = if self.task_filter.is_empty() {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("filter...", Style::default().fg(Gruvbox::FG)),
            ])
        } else {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(Gruvbox::ORANGE)),
                Span::styled(
                    self.task_filter.as_str(),
                    Style::default()
                        .fg(Gruvbox::FG_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        };
        f.render_widget(Paragraph::new(search_text), chunks[1]);

        // Build grouped list items with section headers
        let mut items: Vec<ListItem> = Vec::new();
        let mut prev_group: Option<&TaskGroup> = None;

        for &idx in &self.task_filtered_indices {
            let task = &self.tasks[idx];

            if prev_group != Some(&task.group) {
                if prev_group.is_some() {
                    items.push(ListItem::new(Line::from(""))); // spacer
                }
                let header = match task.group {
                    TaskGroup::Bug => "  ─── Bugs ───",
                    TaskGroup::UserStory => "  ─── User Stories ───",
                    TaskGroup::Task => "  ─── Tasks ───",
                    TaskGroup::Free => "  ─── Free ───",
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    header,
                    Style::default()
                        .fg(Gruvbox::GRAY)
                        .add_modifier(Modifier::DIM),
                ))));
                prev_group = Some(&task.group);
            }

            let (icon, name_color) = if task.window_exists {
                // Existing window — bright color by type
                match task.group {
                    TaskGroup::Bug => ("🐛", Gruvbox::YELLOW),
                    TaskGroup::UserStory => ("📖", Gruvbox::BLUE),
                    TaskGroup::Task => ("✅", Gruvbox::AQUA),
                    TaskGroup::Free => ("💻", Gruvbox::FG),
                }
            } else {
                // AzDo item without local window — dimmer
                match task.group {
                    TaskGroup::Bug => ("🐛", Gruvbox::GRAY),
                    TaskGroup::UserStory => ("📖", Gruvbox::GRAY),
                    TaskGroup::Task => ("✅", Gruvbox::GRAY),
                    TaskGroup::Free => ("💻", Gruvbox::GRAY),
                }
            };

            let suffix = if task.window_exists {
                String::new()
            } else {
                " ⊕ new".to_string()
            };

            items.push(ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{} ", icon), Style::default().fg(name_color)),
                Span::styled(&task.name, Style::default().fg(name_color)),
                Span::styled(suffix, Style::default().fg(Gruvbox::GREEN)),
            ])));
        }

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        self.list_area = chunks[3];
        f.render_stateful_widget(list, chunks[3], &mut self.task_list_state);

        // Bottom bar
        let bottom = if let Some(ref msg) = self.status_msg {
            let frame = if self.loading {
                SPINNER[self.spinner_tick % SPINNER.len()]
            } else {
                "⚠"
            };
            Line::from(Span::styled(
                format!("  {} {}", frame, msg),
                Style::default().fg(Gruvbox::YELLOW),
            ))
        } else {
            Line::from(vec![
                Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
                Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
                Span::styled(" open  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("o", Style::default().fg(Gruvbox::FG)),
                Span::styled(" sessions  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("c", Style::default().fg(Gruvbox::FG)),
                Span::styled(" +copilot  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("t", Style::default().fg(Gruvbox::FG)),
                Span::styled(" +term  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("q", Style::default().fg(Gruvbox::FG)),
                Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
            ])
        };
        f.render_widget(Paragraph::new(bottom), chunks[4]);
    }

    fn render_dashboard(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // help
            ])
            .split(area);

        // Title line — minimal
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  📊 ", Style::default()),
                Span::styled(
                    "Active Sessions",
                    Style::default()
                        .fg(Gruvbox::ORANGE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ({})", self.dashboard_sessions.len()),
                    Style::default().fg(Gruvbox::GRAY),
                ),
            ])),
            chunks[0],
        );

        let items: Vec<ListItem> = self
            .dashboard_sessions
            .iter()
            .map(|entry| {
                let attached_icon = if entry.attached { "▶" } else { " " };
                let name_color = if entry.attached {
                    Gruvbox::GREEN
                } else if entry.work_item_id.is_some() {
                    Gruvbox::FG_BRIGHT
                } else {
                    Gruvbox::FG
                };
                let azdo_tag = entry
                    .work_item_id
                    .map(|id| format!(" #{id}"))
                    .unwrap_or_default();
                let windows_preview: String = entry
                    .windows
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("  {} ", attached_icon),
                            Style::default().fg(Gruvbox::GREEN),
                        ),
                        Span::styled(
                            &entry.session_name,
                            Style::default()
                                .fg(name_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(azdo_tag, Style::default().fg(Gruvbox::BLUE)),
                        Span::styled(
                            format!("  {}w", entry.window_count),
                            Style::default().fg(Gruvbox::GRAY),
                        ),
                    ]),
                    Line::from(Span::styled(
                        format!("      └ {}", windows_preview),
                        Style::default().fg(Gruvbox::GRAY),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        self.list_area = chunks[2];
        f.render_stateful_widget(list, chunks[2], &mut self.dashboard_list_state);

        let help = Line::from(vec![
            Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
            Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
            Span::styled(" attach  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("o", Style::default().fg(Gruvbox::FG)),
            Span::styled(" sessions  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("d", Style::default().fg(Gruvbox::FG)),
            Span::styled(" kill  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("q", Style::default().fg(Gruvbox::FG)),
            Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
        ]);
        f.render_widget(Paragraph::new(help), chunks[3]);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
