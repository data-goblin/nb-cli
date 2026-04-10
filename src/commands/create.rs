// #region Imports
use anyhow::Result;
use reqwest::Client;
use serde_json::json;

use crate::client;
use crate::notebook;
use crate::spinner::{Spinner, DIM, RESET};
// #endregion

// #region Functions

/// Handle `nb create <workspace/name>` command.
/// Creates a new notebook with the specified kernel and optional lakehouse/warehouse bindings.
pub async fn run_create(
    http: &Client,
    reference: &str,
    kernel: &str,
    lakehouse: Option<&str>,
    warehouse: Option<&str>,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;

    let sp = Spinner::start(&format!("Creating '{}'...", nb_name));

    // Resolve lakehouse if specified
    let mut lh_id = None;
    let mut lh_name_resolved = None;
    if let Some(lh) = lakehouse {
        let item = client::resolve_item(http, &ws_id, lh, "Lakehouse").await?;
        lh_id = Some(item.id);
        lh_name_resolved = Some(item.display_name);
    }

    // Resolve warehouse if specified
    let mut wh_id = None;
    let mut wh_name_resolved = None;
    if let Some(wh) = warehouse {
        let item = client::resolve_item(http, &ws_id, wh, "Warehouse").await?;
        wh_id = Some(item.id);
        wh_name_resolved = Some(item.display_name);
    }

    let (_, encoded) = notebook::generate_ipynb(
        kernel,
        lh_id.as_deref(),
        lh_name_resolved.as_deref(),
        wh_id.as_deref(),
        wh_name_resolved.as_deref(),
        &ws_id,
    );

    let body = json!({
        "displayName": nb_name,
        "type": "Notebook",
        "definition": {
            "format": "ipynb",
            "parts": [
                {
                    "path": "notebook-content.ipynb",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    let item = client::create_item(http, &ws_id, &body).await?;
    sp.finish("Created notebook", true).await;

    eprintln!("  {DIM}name{RESET}       {}", item.display_name);
    eprintln!("  {DIM}id{RESET}         {}", item.id);
    eprintln!("  {DIM}workspace{RESET}  {}", ws_id);
    eprintln!("  {DIM}kernel{RESET}     {}", kernel);

    if let Some(ref lh) = lh_name_resolved {
        eprintln!("  {DIM}lakehouse{RESET}  {}", lh);
    }
    if let Some(ref wh) = wh_name_resolved {
        eprintln!("  {DIM}warehouse{RESET}  {}", wh);
    }

    Ok(())
}

// #endregion
