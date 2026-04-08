use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::{AzdoConfig, WorkItem, WorkItemType};
use crate::store::Store;

const CACHE_TTL_MINUTES: i64 = 15;

/// Fetch features (parent work items) from AzDo, with caching
pub fn fetch_features(azdo: &AzdoConfig, store: &Store) -> Result<Vec<WorkItem>> {
    let cache_key = format!("features:{}:{}", azdo.organization, azdo.project);

    // Check cache first
    if let Some(cached) = store.get_cached(&cache_key, CACHE_TTL_MINUTES)? {
        let items: Vec<WorkItem> = serde_json::from_str(&cached)?;
        return Ok(items);
    }

    let pat = get_pat()?;
    let items = query_work_items(azdo, &pat, "Feature")?;

    // Cache the result
    let json = serde_json::to_string(&items)?;
    store.set_cached(&cache_key, &json)?;

    Ok(items)
}

/// Fetch features from AzDo — HTTP only, no Store (safe for tokio::spawn)
pub fn fetch_features_no_cache(azdo: &AzdoConfig) -> Result<Vec<WorkItem>> {
    let pat = get_pat()?;
    query_work_items(azdo, &pat, "Feature")
}

/// Fetch child work items from AzDo — HTTP only, no Store (safe for tokio::spawn)
pub fn fetch_tasks_no_cache(azdo: &AzdoConfig, parent_id: u64) -> Result<Vec<WorkItem>> {
    let pat = get_pat()?;
    query_child_items(azdo, &pat, parent_id)
}

/// Fetch child work items (stories, bugs, tasks) for a feature
pub fn fetch_tasks(azdo: &AzdoConfig, store: &Store, parent_id: u64) -> Result<Vec<WorkItem>> {
    let cache_key = format!("tasks:{}:{}:{}", azdo.organization, azdo.project, parent_id);

    if let Some(cached) = store.get_cached(&cache_key, CACHE_TTL_MINUTES)? {
        let items: Vec<WorkItem> = serde_json::from_str(&cached)?;
        return Ok(items);
    }

    let pat = get_pat()?;
    let items = query_child_items(azdo, &pat, parent_id)?;

    let json = serde_json::to_string(&items)?;
    store.set_cached(&cache_key, &json)?;

    Ok(items)
}

/// Fetch organizations for the authenticated user (used by setup wizard)
pub fn fetch_organizations(pat: &str) -> Result<Vec<String>> {
    // Step 1: Get user profile to obtain memberId
    let profile_url = "https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1";
    let profile_body = curl_get(profile_url, pat)?;

    #[derive(Deserialize)]
    struct Profile { id: String }

    let profile: Profile = serde_json::from_str(&profile_body)
        .context("Failed to parse profile. Check your PAT.")?;

    // Step 2: Get accounts (organizations) for this user
    let accounts_url = format!(
        "https://app.vssps.visualstudio.com/_apis/accounts?memberId={}&api-version=7.1",
        profile.id
    );
    let accounts_body = curl_get(&accounts_url, pat)?;

    #[derive(Deserialize)]
    struct Resp { value: Vec<Acct> }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Acct { account_name: String }

    let resp: Resp = serde_json::from_str(&accounts_body)
        .context("Failed to parse organizations")?;
    let mut names: Vec<String> = resp.value.into_iter().map(|a| a.account_name).collect();
    names.sort();
    Ok(names)
}

/// Fetch projects list (used by setup wizard)
pub fn fetch_projects(org: &str) -> Result<Vec<String>> {
    let pat = get_pat()?;
    fetch_projects_with_pat(org, &pat)
}

/// Fetch projects with an explicit PAT (for wizard before env is configured)
pub fn fetch_projects_with_pat(org: &str, pat: &str) -> Result<Vec<String>> {
    let url = format!(
        "https://dev.azure.com/{}/_apis/projects?api-version=7.1&$top=100",
        org
    );
    let body = curl_get(&url, pat)?;

    #[derive(Deserialize)]
    struct Resp { value: Vec<P> }
    #[derive(Deserialize)]
    struct P { name: String }

    let resp: Resp = serde_json::from_str(&body).context("Failed to parse projects")?;
    let mut names: Vec<String> = resp.value.into_iter().map(|p| p.name).collect();
    names.sort();
    Ok(names)
}

/// Fetch teams for a project (used by setup wizard)
pub fn fetch_teams(org: &str, project: &str) -> Result<Vec<String>> {
    let pat = get_pat()?;
    fetch_teams_with_pat(org, project, &pat)
}

/// Fetch teams with an explicit PAT
pub fn fetch_teams_with_pat(org: &str, project: &str, pat: &str) -> Result<Vec<String>> {
    let url = format!(
        "https://dev.azure.com/{}/{}/_apis/teams?api-version=7.1&$top=100",
        org, project
    );
    let body = curl_get(&url, pat)?;

    #[derive(Deserialize)]
    struct Resp { value: Vec<T> }
    #[derive(Deserialize)]
    struct T { name: String }

    let resp: Resp = serde_json::from_str(&body).context("Failed to parse teams")?;
    let mut names: Vec<String> = resp.value.into_iter().map(|t| t.name).collect();
    names.sort();
    Ok(names)
}

/// Fetch area paths for a project (used by setup wizard)
pub fn fetch_area_paths(org: &str, project: &str) -> Result<Vec<String>> {
    let pat = get_pat()?;
    fetch_area_paths_with_pat(org, project, &pat)
}

/// Fetch area paths with an explicit PAT
pub fn fetch_area_paths_with_pat(org: &str, project: &str, pat: &str) -> Result<Vec<String>> {
    let url = format!(
        "https://dev.azure.com/{}/{}/_apis/wit/classificationnodes/Areas?$depth=3&api-version=7.1",
        org, project
    );
    let body = curl_get(&url, pat)?;

    #[derive(Deserialize)]
    struct Node { name: String, children: Option<Vec<Node>> }

    fn collect(node: &Node, prefix: &str, out: &mut Vec<String>) {
        let full = if prefix.is_empty() {
            node.name.clone()
        } else {
            format!("{}\\{}", prefix, node.name)
        };
        out.push(full.clone());
        if let Some(ref kids) = node.children {
            for child in kids {
                collect(child, &full, out);
            }
        }
    }

    let node: Node = serde_json::from_str(&body).context("Failed to parse area paths")?;
    let mut paths = Vec::new();
    collect(&node, "", &mut paths);
    paths.sort();
    Ok(paths)
}

/// Get PAT from environment
fn get_pat() -> Result<String> {
    std::env::var("AZURE_DEVOPS_PAT")
        .or_else(|_| std::env::var("PILOT_AZDO_PAT"))
        .context("AzDo PAT not found. Set AZURE_DEVOPS_PAT or PILOT_AZDO_PAT environment variable")
}

// ─── HTTP via curl subprocess (bypasses Zscaler TLS interception) ──

fn curl_get(url: &str, pat: &str) -> Result<String> {
    let output = Command::new("curl")
        .args(["-s", "--max-time", "15", "-u", &format!(":{}", pat), url])
        .output()
        .context("Failed to run curl")?;
    if !output.status.success() {
        bail!("curl GET failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).context("Invalid UTF-8 in curl response")
}

fn curl_post_json(url: &str, pat: &str, body: &str) -> Result<String> {
    let output = Command::new("curl")
        .args([
            "-s", "--max-time", "15", "-X", "POST",
            "-u", &format!(":{}", pat),
            "-H", "Content-Type: application/json",
            "-d", body, url,
        ])
        .output()
        .context("Failed to run curl")?;
    if !output.status.success() {
        bail!("curl POST failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).context("Invalid UTF-8 in curl response")
}

// ─── Query helpers ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct WiqlResponse {
    #[serde(rename = "workItems")]
    work_items: Vec<WiqlRef>,
}

#[derive(Debug, Deserialize)]
struct WiqlRef {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct BatchResponse {
    value: Vec<WorkItemResponse>,
}

#[derive(Debug, Deserialize)]
struct WorkItemResponse {
    id: u64,
    fields: WorkItemFields,
}

#[derive(Debug, Deserialize)]
struct WorkItemFields {
    #[serde(rename = "System.Title")]
    title: String,
    #[serde(rename = "System.WorkItemType")]
    work_item_type: String,
    #[serde(rename = "System.State")]
    state: String,
    #[serde(rename = "System.AssignedTo")]
    assigned_to: Option<IdentityRef>,
    #[serde(rename = "System.Description")]
    description: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.AcceptanceCriteria")]
    acceptance_criteria: Option<String>,
    #[serde(rename = "System.Parent")]
    parent: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IdentityRef {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

fn query_work_items(azdo: &AzdoConfig, pat: &str, wit_type: &str) -> Result<Vec<WorkItem>> {
    let mut conditions = vec![format!("[System.WorkItemType] = '{}'", wit_type)];

    if !azdo.filters.states.is_empty() {
        let states: Vec<String> = azdo.filters.states.iter().map(|s| format!("'{}'", s)).collect();
        conditions.push(format!("[System.State] IN ({})", states.join(", ")));
    }

    for area in &azdo.filters.area_paths {
        conditions.push(format!("[System.AreaPath] UNDER '{}'", area));
    }

    if azdo.filters.iteration == "current" {
        conditions.push("[System.IterationPath] = @CurrentIteration".to_string());
    } else if !azdo.filters.iteration.is_empty() {
        conditions.push(format!(
            "[System.IterationPath] UNDER '{}'",
            azdo.filters.iteration
        ));
    }

    let wiql = format!(
        "SELECT [System.Id] FROM WorkItems WHERE {} ORDER BY [System.CreatedDate] DESC",
        conditions.join(" AND ")
    );

    let base_url = format!(
        "https://dev.azure.com/{}/{}",
        azdo.organization, azdo.project
    );

    let wiql_body = serde_json::to_string(&serde_json::json!({ "query": wiql }))?;
    let resp = curl_post_json(
        &format!("{}/_apis/wit/wiql?api-version=7.1", base_url),
        pat,
        &wiql_body,
    )
    .context("WIQL query failed")?;

    let wiql_resp: WiqlResponse =
        serde_json::from_str(&resp).context("Failed to parse WIQL response")?;

    if wiql_resp.work_items.is_empty() {
        return Ok(vec![]);
    }

    let ids: Vec<u64> = wiql_resp.work_items.iter().take(200).map(|w| w.id).collect();
    fetch_items_by_ids(&base_url, pat, &ids)
}

fn query_child_items(azdo: &AzdoConfig, pat: &str, parent_id: u64) -> Result<Vec<WorkItem>> {
    let base_url = format!(
        "https://dev.azure.com/{}/{}",
        azdo.organization, azdo.project
    );

    let wiql = format!(
        "SELECT [System.Id] FROM WorkItems WHERE [System.Parent] = {} AND [System.State] <> 'Closed' AND [System.State] <> 'Removed' ORDER BY [System.WorkItemType], [System.Title]",
        parent_id
    );

    let wiql_body = serde_json::to_string(&serde_json::json!({ "query": wiql }))?;
    let resp = curl_post_json(
        &format!("{}/_apis/wit/wiql?api-version=7.1", base_url),
        pat,
        &wiql_body,
    )?;
    let wiql_resp: WiqlResponse = serde_json::from_str(&resp)?;

    if wiql_resp.work_items.is_empty() {
        return Ok(vec![]);
    }

    let ids: Vec<u64> = wiql_resp.work_items.iter().take(200).map(|w| w.id).collect();
    fetch_items_by_ids(&base_url, pat, &ids)
}

fn fetch_items_by_ids(base_url: &str, pat: &str, ids: &[u64]) -> Result<Vec<WorkItem>> {
    let ids_str: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let url = format!(
        "{}/_apis/wit/workitems?ids={}&fields=System.Title,System.WorkItemType,System.State,System.AssignedTo,System.Description,Microsoft.VSTS.Common.AcceptanceCriteria,System.Parent&api-version=7.1",
        base_url,
        ids_str.join(",")
    );
    let resp_body = curl_get(&url, pat)?;
    let resp: BatchResponse = serde_json::from_str(&resp_body)?;

    Ok(resp
        .value
        .into_iter()
        .map(|wi| WorkItem {
            id: Some(wi.id),
            title: wi.fields.title,
            work_item_type: parse_type(&wi.fields.work_item_type),
            state: wi.fields.state,
            assigned_to: wi.fields.assigned_to.and_then(|a| a.display_name),
            description: wi.fields.description.map(|d| strip_html(&d)),
            acceptance_criteria: wi.fields.acceptance_criteria.map(|a| strip_html(&a)),
            parent_id: wi.fields.parent,
        })
        .collect())
}

fn parse_type(s: &str) -> WorkItemType {
    match s {
        "Feature" => WorkItemType::Feature,
        "User Story" => WorkItemType::UserStory,
        "Bug" => WorkItemType::Bug,
        "Task" => WorkItemType::Task,
        _ => WorkItemType::Free,
    }
}

/// Strip HTML tags and decode common entities → plain text for copilot context
pub fn strip_html(html: &str) -> String {
    let mut result = html.to_string();
    // Convert block elements to newlines
    let block_tags = ["<br>", "<br/>", "<br />", "</p>", "</div>", "</li>", "</tr>"];
    for tag in block_tags {
        result = result.replace(tag, "\n");
    }
    // List items get a dash prefix
    result = result.replace("<li>", "- ");
    // Strip all remaining HTML tags
    let mut out = String::with_capacity(result.len());
    let mut in_tag = false;
    for ch in result.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Decode common HTML entities
    out = out.replace("&amp;", "&");
    out = out.replace("&lt;", "<");
    out = out.replace("&gt;", ">");
    out = out.replace("&quot;", "\"");
    out = out.replace("&#39;", "'");
    out = out.replace("&nbsp;", " ");
    // Collapse multiple blank lines
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim().to_string()
}
