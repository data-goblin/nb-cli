// #region Imports
use anyhow::Result;
use reqwest::Client;
use std::process::Command;

use crate::client;
// #endregion

// #region Functions

/// Handle `nb open <workspace/notebook>` command.
/// Opens the notebook in the default browser via the Fabric web UI.
pub async fn run_open(http: &Client, reference: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let url = format!(
        "https://app.fabric.microsoft.com/groups/{}/synapsenotebooks/{}/?experience=fabric-developer",
        ws_id, nb.id
    );

    println!("  Opening '{}'", nb.display_name);
    println!("  {}", url);

    // Open in default browser
    #[cfg(target_os = "macos")]
    Command::new("open").arg(&url).spawn()?;

    #[cfg(target_os = "linux")]
    Command::new("xdg-open").arg(&url).spawn()?;

    #[cfg(target_os = "windows")]
    Command::new("cmd").args(["/C", "start", &url]).spawn()?;

    Ok(())
}

// #endregion
