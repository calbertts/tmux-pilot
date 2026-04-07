use anyhow::Result;

use crate::config::{AppConfig, WorkItem};

/// Build the copilot command with all flags
pub fn build_command(cfg: &AppConfig, work_item: Option<&WorkItem>) -> String {
    let mut parts = vec![cfg.copilot.bin.clone()];

    if cfg.copilot.yolo {
        parts.push("--yolo".to_string());
    }

    if let Some(ref agent) = cfg.copilot.default_agent {
        parts.push("--agent".to_string());
        parts.push(agent.clone());
    }

    for flag in &cfg.copilot.extra_flags {
        parts.push(flag.clone());
    }

    // Add context prompt if we have a work item
    if let Some(wi) = work_item {
        let prompt = build_prompt(&cfg.copilot.prompt_template, wi);
        parts.push("-i".to_string());
        parts.push(format!("\"{}\"", prompt));
    }

    parts.join(" ")
}

/// Build the context prompt from a template and work item
fn build_prompt(template: &str, wi: &WorkItem) -> String {
    let mut prompt = template.to_string();
    prompt = prompt.replace("{type}", &wi.work_item_type.to_string());
    prompt = prompt.replace("{id}", &wi.id.map(|id| id.to_string()).unwrap_or_default());
    prompt = prompt.replace("{title}", &wi.title);
    prompt = prompt.replace(
        "{description}",
        wi.description.as_deref().unwrap_or("(no description)"),
    );
    prompt = prompt.replace(
        "{acceptance_criteria}",
        wi.acceptance_criteria.as_deref().unwrap_or("(none)"),
    );
    // Escape double quotes for shell safety
    prompt = prompt.replace('"', "'");
    // Collapse newlines to spaces for -i flag (single line)
    prompt = prompt.replace('\n', " \\n ");
    prompt
}

/// Launch copilot in the currently active tmux pane
pub fn launch_in_current_pane(cfg: &AppConfig, work_item: Option<&WorkItem>) -> Result<()> {
    let command = build_command(cfg, work_item);
    crate::tmux::send_keys("", &command)?;
    Ok(())
}

/// Launch copilot in a specific tmux target (session:window.pane)
pub fn launch_in_target(cfg: &AppConfig, target: &str, work_item: Option<&WorkItem>) -> Result<()> {
    let command = build_command(cfg, work_item);
    crate::tmux::send_keys(target, &command)?;
    Ok(())
}

/// Build a resume command for a previous copilot session
pub fn build_resume_command(cfg: &AppConfig, session_id: &str) -> String {
    let mut parts = vec![cfg.copilot.bin.clone()];

    if cfg.copilot.yolo {
        parts.push("--yolo".to_string());
    }

    if let Some(ref agent) = cfg.copilot.default_agent {
        parts.push("--agent".to_string());
        parts.push(agent.clone());
    }

    parts.push(format!("--resume={}", session_id));

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CopilotConfig, WorkItemType};

    fn test_config() -> AppConfig {
        AppConfig {
            copilot: CopilotConfig {
                bin: "copilot".to_string(),
                yolo: true,
                default_agent: Some("my-agent".to_string()),
                extra_flags: vec!["--add-dir".to_string(), "~/code".to_string()],
                auto_launch: true,
                prompt_template: "Working on {type} #{id}: {title}".to_string(),
            },
            azdo: None,
            keybindings: Default::default(),
            notify: Default::default(),
        }
    }

    #[test]
    fn test_build_command_with_work_item() {
        let cfg = test_config();
        let wi = WorkItem {
            id: Some(12345),
            title: "Fix IBAN validation".to_string(),
            work_item_type: WorkItemType::Bug,
            state: "Active".to_string(),
            assigned_to: None,
            description: Some("The IBAN validation fails for NL accounts".to_string()),
            acceptance_criteria: Some("All NL IBANs pass validation".to_string()),
            parent_id: None,
        };

        let cmd = build_command(&cfg, Some(&wi));
        assert!(cmd.contains("--yolo"));
        assert!(cmd.contains("--agent my-agent"));
        assert!(cmd.contains("--add-dir ~/code"));
        assert!(cmd.contains("-i"));
        assert!(cmd.contains("Bug #12345"));
        assert!(cmd.contains("Fix IBAN validation"));
    }

    #[test]
    fn test_build_command_without_work_item() {
        let cfg = test_config();
        let cmd = build_command(&cfg, None);
        assert!(cmd.contains("--yolo"));
        assert!(cmd.contains("--agent my-agent"));
        assert!(!cmd.contains("-i")); // no work item, no -i
    }

    #[test]
    fn test_build_resume_command() {
        let cfg = test_config();
        let cmd = build_resume_command(&cfg, "abc-123");
        assert!(cmd.contains("--resume=abc-123"));
        assert!(cmd.contains("--yolo"));
    }
}
