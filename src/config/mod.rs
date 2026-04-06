mod types;

use std::path::PathBuf;

use anyhow::{Context, Result};

pub use types::*;

/// Returns the path to the user config file: ~/.config/tcs/config.toml
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("tcs")
        .join("config.toml")
}

/// Returns the path to session data: ~/.local/share/tcs/
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tcs")
}

impl AppConfig {
    /// Load config from ~/.config/tcs/config.toml, falling back to defaults.
    /// Then enrich with SIBA_* environment variables for any unset AzDo fields.
    pub fn load() -> Result<Self> {
        let path = config_path();
        let mut config = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config at {}", path.display()))?;
            toml::from_str(&content)
                .with_context(|| format!("Failed to parse config at {}", path.display()))?
        } else {
            tracing::info!("No config file found at {}, using defaults", path.display());
            AppConfig::default()
        };

        config.enrich_from_env();
        Ok(config)
    }

    /// Fill in missing AzDo config from SIBA_* and AZURE_DEVOPS_PAT env vars
    fn enrich_from_env(&mut self) {
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());

        // Auto-create AzDo config from env if not set in TOML
        let azdo = self.azdo.get_or_insert_with(AzdoConfig::default);

        if azdo.organization.is_empty() {
            // SIBA uses nn-bank org; no env var for this, but it's the only org
            if env("SIBA_PROJECT_BACKLOG").is_some() {
                azdo.organization = "nn-bank".to_string();
            }
        }
        if azdo.project.is_empty() {
            if let Some(project) = env("SIBA_PROJECT_BACKLOG") {
                azdo.project = project;
            }
        }
        if azdo.team.is_none() {
            if let Some(team) = env("SIBA_TEAM_NAME") {
                azdo.team = Some(team);
            }
        }
        if azdo.filters.area_paths.is_empty() {
            if let Some(area) = env("SIBA_AREA_PATH") {
                azdo.filters.area_paths = vec![area];
            }
        }

        // If copilot extra_flags don't include --add-dir and SIBA_CODE_PATH is set, add it
        if let Some(code_path) = env("SIBA_CODE_PATH") {
            let has_add_dir = self.copilot.extra_flags.iter().any(|f| f == "--add-dir");
            if !has_add_dir {
                self.copilot.extra_flags.push("--add-dir".to_string());
                self.copilot.extra_flags.push(code_path);
            }
        }

        // Drop azdo section if still unconfigured (no project)
        if let Some(ref azdo) = self.azdo {
            if azdo.project.is_empty() {
                self.azdo = None;
            }
        }
    }

    /// Save current config to disk (creates parent dirs if needed)
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}
