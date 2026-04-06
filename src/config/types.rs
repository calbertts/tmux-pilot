use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub copilot: CopilotConfig,
    pub azdo: Option<AzdoConfig>,
    pub keybindings: KeybindingsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            copilot: CopilotConfig::default(),
            azdo: None,
            keybindings: KeybindingsConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CopilotConfig {
    /// Path to the copilot binary
    pub bin: String,
    /// Always launch with --yolo (auto-approve all actions)
    pub yolo: bool,
    /// Default agent to use (user-specific, not project default)
    pub default_agent: Option<String>,
    /// Extra flags to pass to copilot on every launch
    pub extra_flags: Vec<String>,
    /// Auto-launch copilot when creating new windows
    pub auto_launch: bool,
    /// Prompt template for context injection
    /// Placeholders: {type}, {id}, {title}, {description}, {acceptance_criteria}
    pub prompt_template: String,
    /// Always start copilot in plan mode (prepends [[PLAN]] to prompt)
    pub plan_mode: bool,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            bin: "copilot".to_string(),
            yolo: true,
            default_agent: None,
            extra_flags: vec![],
            auto_launch: true,
            prompt_template: "I'm going to work on {type} #{id}: {title}\n\nDescription:\n{description}\n\nAcceptance Criteria:\n{acceptance_criteria}\n\nDon't take any action yet. Just acknowledge this context.".to_string(),
            plan_mode: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AzdoConfig {
    pub organization: String,
    pub project: String,
    pub team: Option<String>,
    pub filters: AzdoFilters,
}

impl Default for AzdoConfig {
    fn default() -> Self {
        Self {
            organization: String::new(),
            project: String::new(),
            team: None,
            filters: AzdoFilters::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AzdoFilters {
    /// "current" or a specific iteration path
    pub iteration: String,
    /// Work item states to include
    pub states: Vec<String>,
    /// Area paths to filter
    pub area_paths: Vec<String>,
}

impl Default for AzdoFilters {
    fn default() -> Self {
        Self {
            iteration: "current".to_string(),
            states: vec![
                "New".to_string(),
                "Active".to_string(),
                "Resolved".to_string(),
            ],
            area_paths: vec![],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    /// Key for feature selector (prefix + key)
    pub feature_selector: String,
    /// Key for task selector
    pub task_selector: String,
    /// Key for dashboard
    pub dashboard: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            feature_selector: "F".to_string(),
            task_selector: "T".to_string(),
            dashboard: "D".to_string(),
        }
    }
}

/// Represents a work item from Azure DevOps (or a free-form item)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: Option<u64>,
    pub title: String,
    pub work_item_type: WorkItemType,
    pub state: String,
    pub assigned_to: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub parent_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkItemType {
    Feature,
    UserStory,
    Bug,
    Task,
    Free,
}

impl std::fmt::Display for WorkItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkItemType::Feature => write!(f, "Feature"),
            WorkItemType::UserStory => write!(f, "User Story"),
            WorkItemType::Bug => write!(f, "Bug"),
            WorkItemType::Task => write!(f, "Task"),
            WorkItemType::Free => write!(f, "Free"),
        }
    }
}

impl WorkItem {
    /// Get the icon for this work item type
    pub fn icon(&self) -> &str {
        match self.work_item_type {
            WorkItemType::Feature => "🏗",
            WorkItemType::UserStory => "📖",
            WorkItemType::Bug => "🐛",
            WorkItemType::Task => "✅",
            WorkItemType::Free => "📁",
        }
    }

    /// Format as a display string for TUI lists
    pub fn display_label(&self) -> String {
        match self.id {
            Some(id) => format!("{} #{} {}", self.icon(), id, self.title),
            None => format!("{} {}", self.icon(), self.title),
        }
    }
}
