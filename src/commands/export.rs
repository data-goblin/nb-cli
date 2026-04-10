// #region Imports
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::Client;
use std::path::Path;

use crate::client;
use crate::spinner::{Spinner, DIM, RESET};
// #endregion

// #region Functions

/// Handle `nb export <workspace/notebook> -o <path>` command.
/// Exports a notebook definition to a local .ipynb file.
pub async fn run_export(http: &Client, reference: &str, output: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let sp = Spinner::start(&format!("Exporting '{}'...", nb.display_name));

    let def = client::get_definition(http, &ws_id, &nb.id, "ipynb").await?;

    let parts = def
        .definition
        .context("No definition returned")?
        .parts;

    let part = parts
        .iter()
        .find(|p| p.path.contains("notebook-content") || p.path.ends_with(".ipynb"))
        .or_else(|| parts.first())
        .context("No parts in definition")?;

    let decoded = BASE64
        .decode(&part.payload)
        .context("Failed to decode base64 payload")?;

    let json_val: serde_json::Value = serde_json::from_slice(&decoded)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&decoded).into()));
    let pretty = serde_json::to_string_pretty(&json_val)?;

    let out_path = Path::new(output);
    if let Some(parent) = out_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    std::fs::write(out_path, pretty.as_bytes())
        .context("Failed to write output file")?;

    sp.finish(&format!("Exported to {}", output), true).await;
    eprintln!("  {DIM}size{RESET}  {} bytes", pretty.len());

    Ok(())
}

// #endregion
