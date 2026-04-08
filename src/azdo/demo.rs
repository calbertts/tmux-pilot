use crate::config::{WorkItem, WorkItemType};

/// Demo features for --demo mode (generic project management tool)
pub fn demo_features() -> Vec<WorkItem> {
    vec![
        WorkItem {
            id: Some(10001),
            title: "OAuth 2.0 Authentication".to_string(),
            work_item_type: WorkItemType::Feature,
            state: "Active".to_string(),
            assigned_to: Some("Alice Chen".to_string()),
            description: Some("Implement OAuth 2.0 authentication flow with support for Google, GitHub, and Microsoft identity providers. Include token refresh, session management, and role-based access control.".to_string()),
            acceptance_criteria: Some("- Users can sign in with Google, GitHub, or Microsoft\n- Tokens refresh automatically before expiry\n- RBAC with admin, editor, and viewer roles\n- Session timeout after 30 minutes of inactivity".to_string()),
            parent_id: None,
        },
        WorkItem {
            id: Some(10002),
            title: "Real-time Collaboration Engine".to_string(),
            work_item_type: WorkItemType::Feature,
            state: "Active".to_string(),
            assigned_to: Some("Bob Martinez".to_string()),
            description: Some("Build a real-time collaboration system using WebSockets for live document editing, cursor tracking, and presence indicators.".to_string()),
            acceptance_criteria: Some("- Multiple users can edit the same document simultaneously\n- Cursor positions are visible to all collaborators\n- Conflict resolution via CRDT\n- Presence indicators show who is online".to_string()),
            parent_id: None,
        },
        WorkItem {
            id: Some(10003),
            title: "Dashboard Analytics".to_string(),
            work_item_type: WorkItemType::Feature,
            state: "New".to_string(),
            assigned_to: None,
            description: Some("Create an analytics dashboard with charts, KPIs, and exportable reports for project metrics.".to_string()),
            acceptance_criteria: None,
            parent_id: None,
        },
        WorkItem {
            id: Some(10004),
            title: "API Rate Limiting & Throttling".to_string(),
            work_item_type: WorkItemType::Feature,
            state: "Active".to_string(),
            assigned_to: Some("Carol Park".to_string()),
            description: Some("Implement rate limiting middleware with configurable thresholds per API key, IP address, and endpoint.".to_string()),
            acceptance_criteria: Some("- Token bucket algorithm with configurable rates\n- Per-API-key and per-IP limits\n- Return 429 with Retry-After header\n- Admin dashboard for monitoring".to_string()),
            parent_id: None,
        },
        WorkItem {
            id: Some(10005),
            title: "Multi-tenant Data Isolation".to_string(),
            work_item_type: WorkItemType::Feature,
            state: "Resolved".to_string(),
            assigned_to: Some("David Kim".to_string()),
            description: Some("Ensure complete data isolation between tenants using row-level security and schema separation.".to_string()),
            acceptance_criteria: None,
            parent_id: None,
        },
    ]
}

/// Demo tasks (children of feature 10001 — OAuth)
pub fn demo_tasks_auth() -> Vec<WorkItem> {
    vec![
        WorkItem {
            id: Some(20001),
            title: "Google OAuth integration broken on Safari".to_string(),
            work_item_type: WorkItemType::Bug,
            state: "Active".to_string(),
            assigned_to: Some("Alice Chen".to_string()),
            description: Some("The Google OAuth redirect fails silently on Safari 17+ due to ITP blocking third-party cookies.".to_string()),
            acceptance_criteria: Some("- OAuth flow works on Safari 17+\n- No cookie-based workarounds\n- Add integration test for Safari UA".to_string()),
            parent_id: Some(10001),
        },
        WorkItem {
            id: Some(20002),
            title: "Token refresh returns 401 intermittently".to_string(),
            work_item_type: WorkItemType::Bug,
            state: "New".to_string(),
            assigned_to: None,
            description: Some("Under high load, the token refresh endpoint returns 401 instead of a new token. Race condition suspected in the token store.".to_string()),
            acceptance_criteria: None,
            parent_id: Some(10001),
        },
        WorkItem {
            id: Some(20003),
            title: "Add GitHub identity provider".to_string(),
            work_item_type: WorkItemType::UserStory,
            state: "Active".to_string(),
            assigned_to: Some("Alice Chen".to_string()),
            description: Some("As a user, I want to sign in with my GitHub account so that I can use my existing developer identity.".to_string()),
            acceptance_criteria: Some("- GitHub OAuth app configured\n- User profile mapped from GitHub API\n- Avatar synced from GitHub\n- Link/unlink GitHub account".to_string()),
            parent_id: Some(10001),
        },
        WorkItem {
            id: Some(20004),
            title: "Implement RBAC middleware".to_string(),
            work_item_type: WorkItemType::UserStory,
            state: "Active".to_string(),
            assigned_to: Some("Eve Wilson".to_string()),
            description: Some("As an admin, I want role-based access control so that I can restrict actions based on user roles.".to_string()),
            acceptance_criteria: Some("- Three roles: admin, editor, viewer\n- Middleware checks permissions per route\n- 403 returned for unauthorized access\n- Role assignment via admin panel".to_string()),
            parent_id: Some(10001),
        },
        WorkItem {
            id: Some(20005),
            title: "Write OAuth integration tests".to_string(),
            work_item_type: WorkItemType::Task,
            state: "New".to_string(),
            assigned_to: None,
            description: Some("Create comprehensive integration tests for all OAuth flows including edge cases.".to_string()),
            acceptance_criteria: None,
            parent_id: Some(10001),
        },
        WorkItem {
            id: Some(20006),
            title: "Session timeout configuration".to_string(),
            work_item_type: WorkItemType::Task,
            state: "Active".to_string(),
            assigned_to: Some("Alice Chen".to_string()),
            description: Some("Make session timeout configurable per tenant with sensible defaults.".to_string()),
            acceptance_criteria: None,
            parent_id: Some(10001),
        },
    ]
}

/// Demo tasks (children of feature 10002 — Collaboration)
pub fn demo_tasks_collab() -> Vec<WorkItem> {
    vec![
        WorkItem {
            id: Some(20010),
            title: "WebSocket connection drops after idle".to_string(),
            work_item_type: WorkItemType::Bug,
            state: "Active".to_string(),
            assigned_to: Some("Bob Martinez".to_string()),
            description: Some("WebSocket connections are silently dropped after 60s of inactivity. Need heartbeat mechanism.".to_string()),
            acceptance_criteria: Some("- Implement ping/pong heartbeat every 30s\n- Auto-reconnect on disconnect\n- Buffer pending changes during reconnect".to_string()),
            parent_id: Some(10002),
        },
        WorkItem {
            id: Some(20011),
            title: "Implement CRDT for text editing".to_string(),
            work_item_type: WorkItemType::UserStory,
            state: "Active".to_string(),
            assigned_to: Some("Bob Martinez".to_string()),
            description: Some("As a user, I want to edit documents simultaneously with others without conflicts.".to_string()),
            acceptance_criteria: Some("- Yjs-based CRDT implementation\n- Works offline with sync on reconnect\n- Undo/redo per user\n- No data loss on concurrent edits".to_string()),
            parent_id: Some(10002),
        },
        WorkItem {
            id: Some(20012),
            title: "Add presence indicators".to_string(),
            work_item_type: WorkItemType::UserStory,
            state: "New".to_string(),
            assigned_to: None,
            description: Some("As a user, I want to see who else is viewing or editing the document.".to_string()),
            acceptance_criteria: None,
            parent_id: Some(10002),
        },
    ]
}

/// Demo notifications for the notification center view
pub fn demo_notifications() -> Vec<(String, String, String, String)> {
    vec![
        ("Pipeline #4521 succeeded".to_string(), "success".to_string(), "pipeline".to_string(), "All 247 tests passed, coverage 94.2%".to_string()),
        ("PR #189 has merge conflicts".to_string(), "warn".to_string(), "pr-review".to_string(), "Branch feature/oauth-github has conflicts with main".to_string()),
        ("SonarQube: 2 new code smells".to_string(), "info".to_string(), "sonarqube".to_string(), "Detected in src/auth/token.rs and src/auth/session.rs".to_string()),
        ("Build #4519 failed".to_string(), "error".to_string(), "pipeline".to_string(), "Test auth::refresh_token_race timed out after 30s".to_string()),
        ("PR #187 approved by Carol".to_string(), "success".to_string(), "pr-review".to_string(), "Ready to merge: API Rate Limiting implementation".to_string()),
    ]
}
