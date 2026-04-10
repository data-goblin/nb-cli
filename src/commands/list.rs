// #region Imports
use anyhow::Result;
use reqwest::Client;

use crate::client;
// #endregion

// #region Functions

/// Handle `nb list <workspace>` command.
/// Lists all notebooks in the specified workspace with aligned columns.
pub async fn run_list(http: &Client, workspace: &str) -> Result<()> {
    let ws_id = client::resolve_workspace(http, workspace).await?;
    let items = client::list_items(http, &ws_id, "Notebook").await?;

    if items.is_empty() {
        println!("  No notebooks found in workspace '{}'", workspace);
        return Ok(());
    }

    // Calculate column widths
    let max_name = items
        .iter()
        .map(|i| i.display_name.len())
        .max()
        .unwrap_or(4)
        .max(4);

    println!(
        "  {:<width$}  {:<36}  {}",
        "Name",
        "ID",
        "Description",
        width = max_name
    );
    println!(
        "  {:<width$}  {:<36}  {}",
        "-".repeat(max_name),
        "-".repeat(36),
        "-".repeat(20),
        width = max_name
    );

    for item in &items {
        let desc = item.description.as_deref().unwrap_or("");
        println!(
            "  {:<width$}  {:<36}  {}",
            item.display_name,
            item.id,
            desc,
            width = max_name
        );
    }

    println!("\n  {} notebook(s)", items.len());
    Ok(())
}

// #endregion
