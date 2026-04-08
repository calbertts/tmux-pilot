mod app;
mod fuzzy;
mod notifications;
pub mod theme;
mod watchers;

use anyhow::Result;
use crate::config::AppConfig;
use crate::store::Store;

pub use app::App;

/// Run the feature selector TUI
pub async fn run_feature_selector(cfg: &AppConfig, demo: bool, demo_auto: bool) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::FeatureSelector, demo, demo_auto);
    app.run().await
}

/// Run the task selector TUI
pub async fn run_task_selector(cfg: &AppConfig, demo: bool, demo_auto: bool) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::TaskSelector, demo, demo_auto);
    app.run().await
}

/// Run the dashboard TUI
pub async fn run_dashboard(cfg: &AppConfig, demo: bool, demo_auto: bool) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::Dashboard, demo, demo_auto);
    app.run().await
}

/// Run the notification center TUI (synchronous — no network needed)
pub fn run_notifications_sync(store: &Store) -> Result<()> {
    notifications::run(store)
}

/// Run the watchers TUI (synchronous — no network needed)
pub fn run_watchers_sync(store: &Store) -> Result<()> {
    watchers::run(store)
}
