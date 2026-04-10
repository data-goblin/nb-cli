// #region Imports
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::Client;

use crate::client;
// #endregion

// #region Functions

/// Fetch the notebook ipynb JSON from the Fabric API.
/// Returns the parsed JSON value.
async fn fetch_ipynb(http: &Client, ws_id: &str, item_id: &str) -> Result<serde_json::Value> {
    let def = client::get_definition(http, ws_id, item_id, "ipynb").await?;
    let parts = def.definition.context("No definition returned")?.parts;

    let part = parts
        .iter()
        .find(|p| p.path.contains("notebook-content") || p.path.ends_with(".ipynb"))
        .or_else(|| parts.first())
        .context("No parts in definition")?;

    let decoded = BASE64
        .decode(&part.payload)
        .context("Failed to decode base64 payload")?;

    let ipynb: serde_json::Value =
        serde_json::from_slice(&decoded).context("Failed to parse ipynb JSON")?;

    Ok(ipynb)
}


/// Detect the kernel language group from notebook metadata.
/// Falls back to "jupyter_python" if not found.
fn detect_language_group(ipynb: &serde_json::Value) -> String {
    ipynb
        .pointer("/metadata/microsoft/language_group")
        .and_then(|v| v.as_str())
        .unwrap_or("jupyter_python")
        .to_string()
}


/// Handle `nb cells <workspace/notebook>` command.
/// Lists all cells with index, type, and first line of source.
pub async fn run_cells_list(http: &Client, reference: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let ipynb = fetch_ipynb(http, &ws_id, &nb.id).await?;

    let cells = ipynb
        .get("cells")
        .and_then(|c| c.as_array())
        .context("No cells array in notebook")?;

    if cells.is_empty() {
        println!("  No cells in '{}'", nb.display_name);
        return Ok(());
    }

    println!("  {:<5}  {:<10}  {}", "Index", "Type", "First line");
    println!("  {:<5}  {:<10}  {}", "-----", "----------", "----------");

    for (i, cell) in cells.iter().enumerate() {
        let cell_type = cell
            .get("cell_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let first_line = cell_source_string(cell);
        let first_line = first_line.lines().next().unwrap_or("");
        let truncated = if first_line.len() > 80 {
            format!("{}...", &first_line[..77])
        } else {
            first_line.to_string()
        };

        println!("  {:<5}  {:<10}  {}", i, cell_type, truncated);
    }

    println!("\n  {} cell(s)", cells.len());
    Ok(())
}


/// Handle `nb cell <workspace/notebook> <index>` command.
/// Prints the full source of a single cell.
pub async fn run_cell_view(http: &Client, reference: &str, index: usize) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let ipynb = fetch_ipynb(http, &ws_id, &nb.id).await?;

    let cells = ipynb
        .get("cells")
        .and_then(|c| c.as_array())
        .context("No cells array in notebook")?;

    let cell = cells
        .get(index)
        .context(format!("Cell index {} out of range (0..{})", index, cells.len()))?;

    let cell_type = cell
        .get("cell_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let source = cell_source_string(cell);

    println!("  Cell {} ({})", index, cell_type);
    println!("  {}", "-".repeat(40));
    println!("{}", source);

    Ok(())
}


/// Handle `nb cell add <workspace/notebook> --code "<code>" [--markdown] [--at <index>]`.
/// Adds a new cell to the notebook.
pub async fn run_cell_add(
    http: &Client,
    reference: &str,
    code: &str,
    markdown: bool,
    at: Option<usize>,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let mut ipynb = fetch_ipynb(http, &ws_id, &nb.id).await?;
    let language_group = detect_language_group(&ipynb);

    let cell_type = if markdown { "markdown" } else { "code" };

    let source_lines: Vec<serde_json::Value> = code
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let total = code.lines().count();
            if i < total - 1 {
                serde_json::Value::String(format!("{}\n", line))
            } else {
                serde_json::Value::String(line.to_string())
            }
        })
        .collect();

    let mut new_cell = serde_json::json!({
        "cell_type": cell_type,
        "metadata": {
            "microsoft": {
                "language": "python",
                "language_group": language_group
            }
        },
        "source": source_lines
    });

    if cell_type == "code" {
        new_cell["outputs"] = serde_json::json!([]);
        new_cell["execution_count"] = serde_json::Value::Null;
    }

    let cells = ipynb
        .get_mut("cells")
        .and_then(|c| c.as_array_mut())
        .context("No cells array in notebook")?;

    match at {
        Some(pos) => {
            if pos > cells.len() {
                anyhow::bail!(
                    "Insert position {} out of range (0..{})",
                    pos,
                    cells.len()
                );
            }
            cells.insert(pos, new_cell);
            println!("  Inserted {} cell at index {}", cell_type, pos);
        }
        None => {
            let idx = cells.len();
            cells.push(new_cell);
            println!("  Appended {} cell at index {}", cell_type, idx);
        }
    }

    client::update_definition(http, &ws_id, &nb.id, &ipynb).await?;
    println!("  Definition updated.");

    Ok(())
}


/// Handle `nb cell edit <workspace/notebook> <index> --code "<code>"`.
/// Replaces the source of an existing cell.
pub async fn run_cell_edit(
    http: &Client,
    reference: &str,
    index: usize,
    code: &str,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let mut ipynb = fetch_ipynb(http, &ws_id, &nb.id).await?;

    let cells = ipynb
        .get_mut("cells")
        .and_then(|c| c.as_array_mut())
        .context("No cells array in notebook")?;

    let cells_len = cells.len();
    let cell = cells
        .get_mut(index)
        .context(format!("Cell index {} out of range (0..{})", index, cells_len))?;

    let source_lines: Vec<serde_json::Value> = code
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let total = code.lines().count();
            if i < total - 1 {
                serde_json::Value::String(format!("{}\n", line))
            } else {
                serde_json::Value::String(line.to_string())
            }
        })
        .collect();

    cell["source"] = serde_json::Value::Array(source_lines);

    let cell_type = cell
        .get("cell_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    println!("  Updated {} cell at index {}", cell_type, index);

    client::update_definition(http, &ws_id, &nb.id, &ipynb).await?;
    println!("  Definition updated.");

    Ok(())
}


/// Handle `nb cell rm <workspace/notebook> <index>`.
/// Removes a cell at the given index.
pub async fn run_cell_rm(http: &Client, reference: &str, index: usize) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let mut ipynb = fetch_ipynb(http, &ws_id, &nb.id).await?;

    let cells = ipynb
        .get_mut("cells")
        .and_then(|c| c.as_array_mut())
        .context("No cells array in notebook")?;

    if index >= cells.len() {
        anyhow::bail!(
            "Cell index {} out of range (0..{})",
            index,
            cells.len()
        );
    }

    let removed = cells.remove(index);
    let cell_type = removed
        .get("cell_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    println!("  Removed {} cell at index {}", cell_type, index);

    client::update_definition(http, &ws_id, &nb.id, &ipynb).await?;
    println!("  Definition updated.");

    Ok(())
}


/// Extract the source of a cell as a single string.
/// Handles both string and array-of-strings formats.
pub fn cell_source_string(cell: &serde_json::Value) -> String {
    match cell.get("source") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(""),
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

// #endregion
