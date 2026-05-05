use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind, MouseButton},
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
    TaskDetail,
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
    /// Parent work item for the current task view (used when session doesn't exist yet)
    current_parent_work_item: Option<WorkItem>,
    /// Navigation stack for hierarchical drill-down (Ctrl+O pops)
    parent_stack: Vec<Option<WorkItem>>,

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

    // Vim-style: pending 'g' for gg combo
    pending_g: bool,

    // Detail view state
    detail_work_item: Option<WorkItem>,
    detail_scroll: u16,

    // Demo mode: use fake data, no AzDo/tmux
    demo: bool,
    // Auto-animate: synthetic navigation + auto-exit
    demo_auto: bool,
    demo_auto_step: usize,
    demo_auto_timer: std::time::Instant,
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
    pub fn new(cfg: &'a AppConfig, store: Store, view: View, demo: bool, demo_auto: bool) -> Self {
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
            current_parent_work_item: None,
            parent_stack: Vec::new(),
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
            pending_g: false,
            detail_work_item: None,
            detail_scroll: 0,
            demo,
            demo_auto,
            demo_auto_step: 0,
            demo_auto_timer: std::time::Instant::now(),
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
        if self.demo {
            return self.load_demo_data();
        }
        match self.view {
            View::FeatureSelector => self.load_local_features()?,
            View::TaskSelector => self.load_local_tasks()?,
            View::TaskDetail | View::Dashboard => self.load_dashboard()?,
        }
        Ok(())
    }

    /// Load demo data for all views (no tmux/AzDo needed)
    fn load_demo_data(&mut self) -> Result<()> {
        let demo_features = azdo::demo::demo_features();

        match self.view {
            View::FeatureSelector => {
                // 3 features have sessions (Linked), 2 are AzDo-only, 1 free
                self.features = vec![
                    FeatureEntry {
                        name: "#10001 OAuth 2.0 Authentication".to_string(),
                        group: FeatureGroup::Linked,
                        session_exists: true,
                        window_count: 4,
                        work_item: Some(demo_features[0].clone()),
                    },
                    FeatureEntry {
                        name: "#10002 Real-time Collaboration Engine".to_string(),
                        group: FeatureGroup::Linked,
                        session_exists: true,
                        window_count: 3,
                        work_item: Some(demo_features[1].clone()),
                    },
                    FeatureEntry {
                        name: "#10004 API Rate Limiting & Throttling".to_string(),
                        group: FeatureGroup::Linked,
                        session_exists: true,
                        window_count: 2,
                        work_item: Some(demo_features[3].clone()),
                    },
                    FeatureEntry {
                        name: "#10003 Dashboard Analytics".to_string(),
                        group: FeatureGroup::AzdoOnly,
                        session_exists: false,
                        window_count: 0,
                        work_item: Some(demo_features[2].clone()),
                    },
                    FeatureEntry {
                        name: "#10005 Multi-tenant Data Isolation".to_string(),
                        group: FeatureGroup::AzdoOnly,
                        session_exists: false,
                        window_count: 0,
                        work_item: Some(demo_features[4].clone()),
                    },
                    FeatureEntry {
                        name: "scratch-pad".to_string(),
                        group: FeatureGroup::Free,
                        session_exists: true,
                        window_count: 1,
                        work_item: None,
                    },
                ];
                self.sort_features();
                self.update_feature_filter();
                self.feature_list_state.select(self.first_selectable_row());
            }
            View::TaskSelector => {
                let tasks = azdo::demo::demo_tasks_auth();
                self.current_session = Some("#10001 OAuth 2.0 Authentication".to_string());
                self.tasks = vec![
                    TaskEntry {
                        name: tasks[0].display_label(),
                        group: TaskGroup::Bug,
                        window_exists: true,
                        window_index: Some(1),
                        work_item: Some(tasks[0].clone()),
                        copilot_session_id: Some("abc-123".to_string()),
                    },
                    TaskEntry {
                        name: tasks[1].display_label(),
                        group: TaskGroup::Bug,
                        window_exists: false,
                        window_index: None,
                        work_item: Some(tasks[1].clone()),
                        copilot_session_id: None,
                    },
                    TaskEntry {
                        name: tasks[2].display_label(),
                        group: TaskGroup::UserStory,
                        window_exists: true,
                        window_index: Some(2),
                        work_item: Some(tasks[2].clone()),
                        copilot_session_id: Some("def-456".to_string()),
                    },
                    TaskEntry {
                        name: tasks[3].display_label(),
                        group: TaskGroup::UserStory,
                        window_exists: true,
                        window_index: Some(3),
                        work_item: Some(tasks[3].clone()),
                        copilot_session_id: Some("ghi-789".to_string()),
                    },
                    TaskEntry {
                        name: tasks[4].display_label(),
                        group: TaskGroup::Task,
                        window_exists: false,
                        window_index: None,
                        work_item: Some(tasks[4].clone()),
                        copilot_session_id: None,
                    },
                    TaskEntry {
                        name: tasks[5].display_label(),
                        group: TaskGroup::Task,
                        window_exists: true,
                        window_index: Some(4),
                        work_item: Some(tasks[5].clone()),
                        copilot_session_id: Some("jkl-012".to_string()),
                    },
                    TaskEntry {
                        name: "shell".to_string(),
                        group: TaskGroup::Free,
                        window_exists: true,
                        window_index: Some(0),
                        work_item: None,
                        copilot_session_id: None,
                    },
                ];
                self.sort_tasks();
                self.update_task_filter();
                self.task_list_state.select(self.first_selectable_task_row());
            }
            View::TaskDetail | View::Dashboard => {
                self.dashboard_sessions = vec![
                    DashboardEntry {
                        session_name: "#10001 OAuth 2.0 Authentication".to_string(),
                        window_count: 4,
                        attached: true,
                        windows: vec![
                            "shell".to_string(),
                            "🐛 #20001 Google OAuth Safari".to_string(),
                            "📖 #20003 GitHub identity provider".to_string(),
                            "📖 #20004 RBAC middleware".to_string(),
                        ],
                        work_item_id: Some(10001),
                    },
                    DashboardEntry {
                        session_name: "#10002 Real-time Collaboration".to_string(),
                        window_count: 3,
                        attached: false,
                        windows: vec![
                            "shell".to_string(),
                            "🐛 #20010 WebSocket idle drops".to_string(),
                            "📖 #20011 CRDT text editing".to_string(),
                        ],
                        work_item_id: Some(10002),
                    },
                    DashboardEntry {
                        session_name: "#10004 API Rate Limiting".to_string(),
                        window_count: 2,
                        attached: false,
                        windows: vec!["shell".to_string(), "📖 Token bucket impl".to_string()],
                        work_item_id: Some(10004),
                    },
                    DashboardEntry {
                        session_name: "scratch-pad".to_string(),
                        window_count: 1,
                        attached: false,
                        windows: vec!["shell".to_string()],
                        work_item_id: None,
                    },
                ];
                self.dashboard_list_state.select(Some(0));
            }
        }
        Ok(())
    }

    /// Spawn background AzDo fetch if configured.
    fn start_azdo_fetch(&mut self) {
        // Demo mode already has all data loaded
        if self.demo {
            return;
        }

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
                // When drilling into children (parent_stack not empty), use the current parent work item.
                // Otherwise fall back to session mapping for the top-level feature.
                let parent_id = if !self.parent_stack.is_empty() {
                    self.current_parent_work_item.as_ref().and_then(|wi| wi.id)
                } else {
                    self.store
                        .get_session_mapping(&session_name)
                        .ok()
                        .flatten()
                        .and_then(|m| m.work_item_id)
                        .or_else(|| self.current_parent_work_item.as_ref().and_then(|wi| wi.id))
                };

                if let Some(pid) = parent_id {
                    // Check cache first
                    let cache_key = format!(
                        "tasks:{}:{}:{}",
                        azdo_cfg.organization, azdo_cfg.project, pid
                    );
                    if let Some(cached) = self.store.get_cached(&cache_key, 15).ok().flatten() {
                        if let Ok(items) = serde_json::from_str::<Vec<WorkItem>>(&cached) {
                            if !items.is_empty() {
                                self.merge_azdo_result(AzdoFetchResult::Tasks(Ok(items)));
                                return;
                            }
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
            View::TaskDetail | View::Dashboard => {}
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

                // Update state/description on existing local features from AzDo data
                for wi in azdo_features.iter() {
                    if let Some(feature) = self.features.iter_mut().find(|f| {
                        f.work_item.as_ref().and_then(|w| w.id) == wi.id
                    }) {
                        if let Some(ref mut existing_wi) = feature.work_item {
                            existing_wi.state = wi.state.clone();
                            if existing_wi.description.is_none() {
                                existing_wi.description.clone_from(&wi.description);
                            }
                            if existing_wi.acceptance_criteria.is_none() {
                                existing_wi.acceptance_criteria.clone_from(&wi.acceptance_criteria);
                            }
                        }
                    }
                }

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
                // Cache for next time (skip empty results to avoid poisoning cache)
                if !azdo_tasks.is_empty() {
                    if let Some(ref azdo_cfg) = self.cfg.azdo {
                        let parent_id = if !self.parent_stack.is_empty() {
                            self.current_parent_work_item.as_ref().and_then(|wi| wi.id)
                        } else {
                            self.current_session.as_ref()
                                .and_then(|session| self.store.get_session_mapping(session).ok().flatten())
                                .and_then(|m| m.work_item_id)
                                .or_else(|| self.current_parent_work_item.as_ref().and_then(|wi| wi.id))
                        };
                        if let Some(pid) = parent_id {
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

                // Update state/description on existing local tasks from AzDo data
                for wi in azdo_tasks.iter() {
                    if let Some(task) = self.tasks.iter_mut().find(|t| {
                        t.work_item.as_ref().and_then(|w| w.id) == wi.id
                    }) {
                        if let Some(ref mut existing_wi) = task.work_item {
                            existing_wi.state = wi.state.clone();
                            if existing_wi.description.is_none() {
                                existing_wi.description.clone_from(&wi.description);
                            }
                            if existing_wi.acceptance_criteria.is_none() {
                                existing_wi.acceptance_criteria.clone_from(&wi.acceptance_criteria);
                            }
                        }
                    }
                }

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
        // When drilling into sub-levels (parent_stack not empty), start with empty list.
        // Only AzDo fetch will populate children — there are no tmux windows at sub-levels.
        if !self.parent_stack.is_empty() {
            self.tasks = Vec::new();
            self.sort_tasks();
            self.update_task_filter();
            self.task_list_state.select(None);
            return Ok(());
        }

        // Top-level: populate from tmux windows
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
        // Reset auto-animation timer after data is loaded
        self.demo_auto_timer = std::time::Instant::now();

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

            // Demo auto-animation: synthetic key events on a timer
            if self.demo_auto {
                self.tick_demo_auto();
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

    /// Drive demo auto-animation: move cursor, pause, then exit
    fn tick_demo_auto(&mut self) {
        let elapsed = self.demo_auto_timer.elapsed();
        // Script: (delay_ms, action)
        // Wait 1s, then Down every 350ms x5, then Up every 350ms x3, wait 1.5s, quit
        let script: &[(u64, i8)] = &[
            (1000, 0),   // initial pause
            (1350, 1),   // down
            (1700, 1),
            (2050, 1),
            (2400, 1),
            (2750, 1),
            (3100, -1),  // up
            (3450, -1),
            (3800, -1),
            (5300, 99),  // quit
        ];
        let ms = elapsed.as_millis() as u64;
        while self.demo_auto_step < script.len() && ms >= script[self.demo_auto_step].0 {
            match script[self.demo_auto_step].1 {
                1 => self.move_cursor_down(),
                -1 => self.move_cursor_up(),
                99 => self.running = false,
                _ => {}
            }
            self.demo_auto_step += 1;
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        // Handle text input mode
        if self.input_mode == InputMode::TextInput {
            self.pending_g = false;
            return self.handle_text_input_key(key.code);
        }

        // Handle 'gg' combo: second 'g' after pending
        if self.pending_g {
            self.pending_g = false;
            if key.code == KeyCode::Char('g') {
                self.jump_to_first();
                return Ok(());
            }
            // Not 'g' — the first 'g' was just a filter char, process both
            // First 'g' was consumed, so we add it to filter if applicable
            self.dispatch_char_to_filter('g');
        }

        // Ctrl+O: go back in hierarchy
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
            match self.view {
                View::TaskSelector | View::TaskDetail => {
                    if let Some(prev_parent) = self.parent_stack.pop() {
                        // Go up one level in the hierarchy
                        self.current_parent_work_item = prev_parent;
                        let session = self.current_session.clone();
                        return self.switch_to_view_with_session(View::TaskSelector, session);
                    }
                    // Stack empty: go back to feature selector
                    return self.switch_to_view(View::FeatureSelector);
                }
                View::FeatureSelector => return self.switch_to_view(View::TaskSelector),
                View::Dashboard => return self.switch_to_view(View::FeatureSelector),
            }
        }

        // G (shift+g): jump to last item
        if key.code == KeyCode::Char('G') {
            self.jump_to_last();
            return Ok(());
        }

        // R (shift+r): force refresh — clear cache and re-fetch from AzDo
        if key.code == KeyCode::Char('R') {
            self.store.clear_cache().ok();
            self.start_azdo_fetch();
            self.status_msg = Some("Refreshing from AzDo...".to_string());
            return Ok(());
        }

        // First 'g': start gg combo (only in views with filter, not in dashboard)
        if key.code == KeyCode::Char('g') {
            self.pending_g = true;
            return Ok(());
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.running = false;
            }
            _ => match self.view {
                View::FeatureSelector => self.handle_feature_key(key)?,
                View::TaskSelector => self.handle_task_key(key)?,
                View::TaskDetail => self.handle_detail_key(key.code)?,
                View::Dashboard => self.handle_dashboard_key(key.code)?,
            },
        }
        Ok(())
    }

    /// Jump selection to the first selectable item
    fn jump_to_first(&mut self) {
        match self.view {
            View::FeatureSelector => {
                self.feature_list_state
                    .select(self.first_selectable_row());
            }
            View::TaskSelector => {
                self.task_list_state
                    .select(self.first_selectable_task_row());
            }
            View::TaskDetail => {
                self.detail_scroll = 0;
            }
            View::Dashboard => {
                if !self.dashboard_sessions.is_empty() {
                    self.dashboard_list_state.select(Some(0));
                }
            }
        }
    }

    /// Jump selection to the last selectable item
    fn jump_to_last(&mut self) {
        match self.view {
            View::FeatureSelector => {
                let last = self.visual_map.iter().rposition(|e| e.is_some());
                if let Some(pos) = last {
                    self.feature_list_state.select(Some(pos));
                }
            }
            View::TaskSelector => {
                let last = self.task_visual_map.iter().rposition(|e| e.is_some());
                if let Some(pos) = last {
                    self.task_list_state.select(Some(pos));
                }
            }
            View::TaskDetail => {} // no-op in detail
            View::Dashboard => {
                let len = self.dashboard_sessions.len();
                if len > 0 {
                    self.dashboard_list_state.select(Some(len - 1));
                }
            }
        }
    }

    /// Push a char into the current view's filter (used when pending_g falls through)
    fn dispatch_char_to_filter(&mut self, c: char) {
        match self.view {
            View::FeatureSelector => {
                self.feature_filter.push(c);
                self.update_feature_filter();
                self.feature_list_state
                    .select(self.first_selectable_row());
            }
            View::TaskSelector => {
                self.task_filter.push(c);
                self.update_task_filter();
                self.task_list_state
                    .select(self.first_selectable_task_row());
            }
            View::TaskDetail | View::Dashboard => {} // no filter
        }
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
            View::TaskDetail => self.detail_scroll as usize,
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
            View::TaskDetail => {} // no click in detail
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
        let prev_feature_sel = self.feature_list_state.selected();
        let prev_view = self.view.clone();
        self.view = target.clone();
        self.loading = false;
        self.status_msg = None;
        self.azdo_rx = None;
        if let Some(name) = session {
            self.current_session = Some(name);
        }
        self.load_local_data()?;
        // Restore feature selection when returning to FeatureSelector
        if target == View::FeatureSelector && prev_view != View::FeatureSelector {
            if let Some(sel) = prev_feature_sel {
                if sel < self.visual_map.len() && self.visual_map[sel].is_some() {
                    self.feature_list_state.select(Some(sel));
                }
            }
        }
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

    fn handle_feature_key(&mut self, key: KeyEvent) -> Result<()> {
        // Ctrl+N: create new free session
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('n') {
            self.input_mode = InputMode::TextInput;
            self.text_input_label = "New session name".to_string();
            self.text_input_action = TextInputAction::CreateFreeSession;
            return Ok(());
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(&View::FeatureSelector),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(&View::FeatureSelector),
            KeyCode::Enter => self.select_feature()?,
            KeyCode::Char('o') => {
                // Navigate into children: open task selector for selected feature
                let selected_feature = self.feature_list_state.selected()
                    .and_then(|sel| self.visual_map.get(sel).copied().flatten())
                    .map(|idx| self.features[idx].clone());
                let session = selected_feature.as_ref().map(|f| f.name.clone());
                let parent_wi = selected_feature.and_then(|f| f.work_item);
                self.parent_stack.clear();
                self.current_parent_work_item = parent_wi;
                self.switch_to_view_with_session(View::TaskSelector, session)?;
            }
            KeyCode::Char('d') => {
                // Show detail for selected feature
                let selected_feature = self.feature_list_state.selected()
                    .and_then(|sel| self.visual_map.get(sel).copied().flatten())
                    .map(|idx| self.features[idx].clone());
                if let Some(ref feature) = selected_feature {
                    if let Some(ref wi) = feature.work_item {
                        self.detail_work_item = Some(wi.clone());
                        self.detail_scroll = 0;
                        self.view = View::TaskDetail;
                    }
                }
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

    fn handle_task_key(&mut self, key: KeyEvent) -> Result<()> {
        // Ctrl+N: create new copilot window
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('n') {
            self.do_create_copilot_window(None)?;
            return Ok(());
        }
        // Ctrl+T: create named terminal window
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            self.input_mode = InputMode::TextInput;
            self.text_input_label = "New terminal window name".to_string();
            self.text_input_action = TextInputAction::CreateNamedWindow;
            return Ok(());
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(&View::TaskSelector),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(&View::TaskSelector),
            KeyCode::Enter => self.select_task()?,
            KeyCode::Char('d') => {
                // Show detail view for selected task
                if let Some(selected) = self.task_list_state.selected() {
                    if let Some(Some(idx)) = self.task_visual_map.get(selected).copied() {
                        if let Some(ref wi) = self.tasks[idx].work_item {
                            self.detail_work_item = Some(wi.clone());
                            self.detail_scroll = 0;
                            self.view = View::TaskDetail;
                        }
                    }
                }
            }
            KeyCode::Char('o') => {
                // Navigate into children of selected task
                if let Some(selected) = self.task_list_state.selected() {
                    if let Some(Some(idx)) = self.task_visual_map.get(selected).copied() {
                        let task = &self.tasks[idx];
                        if let Some(ref wi) = task.work_item {
                            // Push current parent to stack for back navigation
                            self.parent_stack.push(self.current_parent_work_item.clone());
                            let session = self.current_session.clone();
                            self.current_parent_work_item = Some(wi.clone());
                            self.switch_to_view_with_session(View::TaskSelector, session)?;
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if self.task_filter.is_empty() && !self.parent_stack.is_empty() {
                    // Navigate back up the hierarchy
                    if let Some(prev_parent) = self.parent_stack.pop() {
                        self.current_parent_work_item = prev_parent;
                        let session = self.current_session.clone();
                        self.switch_to_view_with_session(View::TaskSelector, session)?;
                    }
                } else if self.task_filter.is_empty() {
                    // At top level, go back to features
                    self.switch_to_view(View::FeatureSelector)?;
                } else {
                    self.task_filter.pop();
                    self.update_task_filter();
                    self.task_list_state
                        .select(self.first_selectable_task_row());
                }
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

    fn handle_detail_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
            KeyCode::Backspace => {
                // Go back to task selector
                self.view = View::TaskSelector;
            }
            KeyCode::Enter => {
                // Go to session from detail view
                self.view = View::TaskSelector;
                self.select_task()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn move_cursor_down(&mut self) {
        let view = self.view.clone();
        self.move_selection_down(&view);
    }

    fn move_cursor_up(&mut self) {
        let view = self.view.clone();
        self.move_selection_up(&view);
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
            View::TaskDetail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
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
            View::TaskDetail => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
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
        if self.demo {
            self.status_msg = Some("Demo mode — select disabled".to_string());
            return Ok(());
        }
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
        if self.demo {
            self.status_msg = Some("Demo mode — select disabled".to_string());
            return Ok(());
        }
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

                // Ensure session exists — create it if needed (AzDo-only feature)
                let session_exists = tmux::session_exists(&session).unwrap_or(false);
                if !session_exists {
                    tmux::create_session(&session, None)?;

                    // Persist session mapping from parent work item
                    let parent_wi = &self.current_parent_work_item;
                    self.store.save_session_mapping(&SessionMapping {
                        session_name: session.clone(),
                        work_item_id: parent_wi.as_ref().and_then(|wi| wi.id),
                        work_item_title: parent_wi.as_ref().map(|wi| wi.title.clone()),
                        work_item_type: parent_wi
                            .as_ref()
                            .map(|wi| wi.work_item_type.to_string()),
                        template: None,
                        created_at: String::new(),
                    })?;
                }

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

                    if session_exists {
                        tmux::create_window(&session, &win_name, None)?;
                    } else {
                        // Session just created — rename the default first window
                        tmux::rename_window(&session, 1, &win_name)?;
                    }

                    // Launch copilot with work item context
                    if self.cfg.copilot.auto_launch {
                        let target = format!("{}:{}", session, win_name);
                        copilot::launch_in_target(self.cfg, &target, task.work_item.as_ref())?;
                    }

                    // Persist window mapping
                    self.store.save_window_mapping(&WindowMapping {
                        session_name: session.clone(),
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

                tmux::switch_session(&session)?;
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
            View::TaskDetail => self.render_task_detail(f, area),
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
            .style(Style::default().fg(Gruvbox::FG_BRIGHT).bg(Gruvbox::BG_POPUP))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Gruvbox::ORANGE).bg(Gruvbox::BG_POPUP))
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
                Constraint::Length(1), // footer
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

            // State badge from AzDo
            let state_spans = feature
                .work_item
                .as_ref()
                .filter(|wi| !wi.state.is_empty())
                .map(|wi| {
                    let (symbol, color) = state_badge(&wi.state);
                    vec![Span::styled(format!(" {}", symbol), Style::default().fg(color))]
                })
                .unwrap_or_default();

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(format!("{} ", icon), Style::default().fg(name_color)),
                Span::styled(&feature.name, Style::default().fg(name_color)),
                Span::styled(format!("  {}", detail), Style::default().fg(detail_color)),
            ];
            spans.extend(state_spans);
            items.push(ListItem::new(Line::from(spans)));
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
        if let Some(ref msg) = self.status_msg {
            let frame = if self.loading {
                SPINNER[self.spinner_tick % SPINNER.len()]
            } else {
                "⚠"
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  {} {}", frame, msg),
                    Style::default().fg(Gruvbox::YELLOW),
                ))),
                chunks[3],
            );
        } else {
            let help = vec![
                Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
                Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
                Span::styled(" open  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("o", Style::default().fg(Gruvbox::FG)),
                Span::styled(" tasks  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("d", Style::default().fg(Gruvbox::FG)),
                Span::styled(" detail  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("R", Style::default().fg(Gruvbox::FG)),
                Span::styled(" refresh  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("^n", Style::default().fg(Gruvbox::FG)),
                Span::styled(" new  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("q", Style::default().fg(Gruvbox::FG)),
                Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
            ];
            render_footer(f, chunks[3], help, state_legend(false));
        }
    }

    fn render_task_selector(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // session title
                Constraint::Length(1), // search
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // footer
            ])
            .split(area);

        // Session title with hierarchy breadcrumb
        let session_title = self
            .current_session
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let mut title_spans = vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                &session_title,
                Style::default()
                    .fg(Gruvbox::ORANGE)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        // Show current parent breadcrumb when drilling into children
        if let Some(ref parent_wi) = self.current_parent_work_item {
            let depth = self.parent_stack.len();
            if depth > 0 {
                // We're viewing children of a child (not the top-level feature)
                let label = parent_wi.id
                    .map(|id| format!("  ▸ #{} {}", id, parent_wi.title))
                    .unwrap_or_else(|| format!("  ▸ {}", parent_wi.title));
                title_spans.push(Span::styled(label, Style::default().fg(Gruvbox::AQUA)));
            }
        }
        f.render_widget(
            Paragraph::new(Line::from(title_spans)),
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

            // State badge from AzDo
            let state_spans = task
                .work_item
                .as_ref()
                .filter(|wi| !wi.state.is_empty())
                .map(|wi| {
                    let (symbol, color) = state_badge(&wi.state);
                    vec![Span::styled(format!(" {}", symbol), Style::default().fg(color))]
                })
                .unwrap_or_default();

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(format!("{} ", icon), Style::default().fg(name_color)),
                Span::styled(&task.name, Style::default().fg(name_color)),
                Span::styled(suffix, Style::default().fg(Gruvbox::GREEN)),
            ];
            spans.extend(state_spans);
            items.push(ListItem::new(Line::from(spans)));
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

        // Footer
        if let Some(ref msg) = self.status_msg {
            let frame = if self.loading {
                SPINNER[self.spinner_tick % SPINNER.len()]
            } else {
                "⚠"
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  {} {}", frame, msg),
                    Style::default().fg(Gruvbox::YELLOW),
                ))),
                chunks[4],
            );
        } else {
            let mut help = vec![
                Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
                Span::styled(" move  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
                Span::styled(" go  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("o", Style::default().fg(Gruvbox::FG)),
                Span::styled(" children  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("d", Style::default().fg(Gruvbox::FG)),
                Span::styled(" detail  ", Style::default().fg(Gruvbox::GRAY)),
            ];
            if !self.parent_stack.is_empty() {
                help.push(Span::styled("⌫", Style::default().fg(Gruvbox::FG)));
                help.push(Span::styled(" back  ", Style::default().fg(Gruvbox::GRAY)));
            }
            help.extend([
                Span::styled("R", Style::default().fg(Gruvbox::FG)),
                Span::styled(" refresh  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("^n", Style::default().fg(Gruvbox::FG)),
                Span::styled(" +copilot  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("^t", Style::default().fg(Gruvbox::FG)),
                Span::styled(" +term  ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled("q", Style::default().fg(Gruvbox::FG)),
                Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
            ]);
            render_footer(f, chunks[4], help, state_legend(true));
        }
    }

    fn render_task_detail(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // content
                Constraint::Length(1), // footer
            ])
            .split(area);

        let wi = match &self.detail_work_item {
            Some(wi) => wi,
            None => {
                self.view = View::TaskSelector;
                return;
            }
        };

        // Title line
        let (state_sym, state_color) = state_badge(&wi.state);
        let type_str = wi.work_item_type.to_string();
        let id_str = wi.id.map(|id| format!(" #{}", id)).unwrap_or_default();
        let title_spans = vec![
            Span::styled(format!("  {} ", wi.icon()), Style::default()),
            Span::styled(
                &type_str,
                Style::default().fg(Gruvbox::GRAY),
            ),
            Span::styled(
                &id_str,
                Style::default().fg(Gruvbox::BLUE),
            ),
            Span::styled(
                format!(" {}", wi.title),
                Style::default()
                    .fg(Gruvbox::FG_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", state_sym), Style::default().fg(state_color)),
        ];
        f.render_widget(Paragraph::new(Line::from(title_spans)), chunks[0]);

        // Content: description + acceptance criteria
        let mut lines: Vec<Line> = vec![];

        // Description section
        lines.push(Line::from(Span::styled(
            "  Description",
            Style::default()
                .fg(Gruvbox::ORANGE)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  ─────────────────────────────────────────",
            Style::default().fg(Gruvbox::DARK_GRAY),
        )));
        if let Some(ref desc) = wi.description {
            let clean = azdo::strip_html(desc);
            for line in clean.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(Gruvbox::FG),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (no description)",
                Style::default().fg(Gruvbox::GRAY),
            )));
        }

        lines.push(Line::from(""));

        // Acceptance Criteria section
        lines.push(Line::from(Span::styled(
            "  Acceptance Criteria",
            Style::default()
                .fg(Gruvbox::ORANGE)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  ─────────────────────────────────────────",
            Style::default().fg(Gruvbox::DARK_GRAY),
        )));
        if let Some(ref ac) = wi.acceptance_criteria {
            let clean = azdo::strip_html(ac);
            for line in clean.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(Gruvbox::FG),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Gruvbox::GRAY),
            )));
        }

        // Assigned to
        if let Some(ref assigned) = wi.assigned_to {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Assigned: ", Style::default().fg(Gruvbox::GRAY)),
                Span::styled(assigned, Style::default().fg(Gruvbox::FG)),
            ]));
        }

        let content = Paragraph::new(lines)
            .scroll((self.detail_scroll, 0));
        f.render_widget(content, chunks[2]);

        // Footer
        let help = vec![
            Span::styled("  ↑↓", Style::default().fg(Gruvbox::FG)),
            Span::styled(" scroll  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("⏎", Style::default().fg(Gruvbox::FG)),
            Span::styled(" go to session  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("⌫", Style::default().fg(Gruvbox::FG)),
            Span::styled(" back  ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("q", Style::default().fg(Gruvbox::FG)),
            Span::styled(" quit", Style::default().fg(Gruvbox::GRAY)),
        ];
        let legend = state_legend(false);
        render_footer(f, chunks[3], help, legend);
    }

    fn render_dashboard(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
                Constraint::Length(1), // footer
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

        let help = vec![
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
        ];
        let legend = vec![
            Span::styled("▶", Style::default().fg(Gruvbox::GREEN)),
            Span::styled("attached ", Style::default().fg(Gruvbox::GRAY)),
            Span::styled("#", Style::default().fg(Gruvbox::BLUE)),
            Span::styled("azdo", Style::default().fg(Gruvbox::GRAY)),
        ];
        render_footer(f, chunks[3], help, legend);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

/// Return (emoji, color) for an AzDo work item state
fn state_badge(state: &str) -> (&str, ratatui::style::Color) {
    match state {
        "New" => ("○", Gruvbox::BLUE),
        "Active" => ("●", Gruvbox::GREEN),
        "Resolved" => ("◉", Gruvbox::AQUA),
        "Closed" => ("✔", Gruvbox::GRAY),
        "Removed" => ("✘", Gruvbox::GRAY),
        _ if state.is_empty() => ("", Gruvbox::GRAY),
        _ => ("◌", Gruvbox::YELLOW),
    }
}

/// Build a single footer line: shortcuts left-aligned, legend right-aligned
fn render_footer(f: &mut Frame, area: Rect, help_spans: Vec<Span>, legend_spans: Vec<Span>) {
    render_footer_pub(f, area, help_spans, legend_spans);
}

/// Public version for use by other TUI modules
pub fn render_footer_pub(f: &mut Frame, area: Rect, help_spans: Vec<Span>, legend_spans: Vec<Span>) {
    let help_width: usize = help_spans.iter().map(|s| s.content.chars().count()).sum();
    let legend_width: usize = legend_spans.iter().map(|s| s.content.chars().count()).sum();
    let total = help_width + legend_width;
    let gap = if area.width as usize > total + 1 {
        area.width as usize - total - 1
    } else {
        1
    };

    let mut spans = help_spans;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(legend_spans);

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Legend spans for state symbols (optionally with type icons)
fn state_legend(show_types: bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span> = vec![];

    spans.push(Span::styled("○", Style::default().fg(Gruvbox::BLUE)));
    spans.push(Span::styled("new ", Style::default().fg(Gruvbox::GRAY)));
    spans.push(Span::styled("●", Style::default().fg(Gruvbox::GREEN)));
    spans.push(Span::styled("act ", Style::default().fg(Gruvbox::GRAY)));
    spans.push(Span::styled("◉", Style::default().fg(Gruvbox::AQUA)));
    spans.push(Span::styled("res ", Style::default().fg(Gruvbox::GRAY)));
    spans.push(Span::styled("✔", Style::default().fg(Gruvbox::GRAY)));
    spans.push(Span::styled("cls", Style::default().fg(Gruvbox::GRAY)));

    if show_types {
        spans.push(Span::styled(" │ ", Style::default().fg(Gruvbox::DARK_GRAY)));
        spans.push(Span::styled("📖", Style::default()));
        spans.push(Span::styled("story ", Style::default().fg(Gruvbox::GRAY)));
        spans.push(Span::styled("🐛", Style::default()));
        spans.push(Span::styled("bug ", Style::default().fg(Gruvbox::GRAY)));
        spans.push(Span::styled("✅", Style::default()));
        spans.push(Span::styled("task", Style::default().fg(Gruvbox::GRAY)));
    }

    spans.push(Span::styled(" │ ", Style::default().fg(Gruvbox::DARK_GRAY)));
    spans.push(Span::styled("⊕", Style::default().fg(Gruvbox::GREEN)));
    spans.push(Span::styled("azdo", Style::default().fg(Gruvbox::GRAY)));

    spans
}
