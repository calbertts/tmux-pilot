mod app;
mod fuzzy;
pub mod theme;

use anyhow::Result;
use crate::config::AppConfig;
use crate::store::Store;

pub use app::App;

/// Run the feature selector TUI
pub async fn run_feature_selector(cfg: &AppConfig) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::FeatureSelector);
    app.run().await
}

/// Run the task selector TUI
pub async fn run_task_selector(cfg: &AppConfig) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::TaskSelector);
    app.run().await
}

/// Run the dashboard TUI
pub async fn run_dashboard(cfg: &AppConfig) -> Result<()> {
    let store = Store::open()?;
    let mut app = App::new(cfg, store, app::View::Dashboard);
    app.run().await
}
