// #region Imports
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::Client;
use std::io::Read as _;

use crate::auth;
use crate::client;
use crate::spinner::{Spinner, DIM, GREEN, RED, RESET, ERASE};
use super::cells::cell_source_string;
// #endregion

// #region Variables
const LIVY_API_VERSION: &str = "2023-12-01";
// #endregion


/// Handle `nb exec code <workspace/lakehouse> <code>` command.
/// Ephemeral execution: creates a session, runs code, prints output, cleans up.
/// No notebook artifact required. Pass `-` as code to read from stdin.
pub async fn run_exec_quick(
    http: &Client,
    reference: &str,
    code: &str,
) -> Result<()> {
    let (ws_name, lh_name) = client::parse_ref(reference)
        .context("Expected format: \"Workspace/Lakehouse\" or \"Workspace.Workspace/LH.Lakehouse\"\n  nb exec code \"MyWS/MyLH.Lakehouse\" \"print('hello')\"")?;

    // Strip .Lakehouse suffix if present
    let lh_clean = lh_name.trim_end_matches(".Lakehouse");

    // Read code from stdin if "-"
    let exec_code = if code == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)
            .context("Failed to read code from stdin")?;
        buf
    } else {
        code.to_string()
    };

    if exec_code.trim().is_empty() {
        anyhow::bail!("No code to execute.\n  nb exec code \"Workspace/Lakehouse\" \"print('hello')\"");
    }

    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let lh = client::resolve_item(http, &ws_id, lh_clean, "Lakehouse").await?;
    let session_name = format!("nb-cli-{}", lh_clean);

    run_in_session(http, &ws_id, &lh.id, "pyspark", &session_name, &exec_code).await
}


/// Handle `nb exec cell <workspace/notebook> <index>` command.
/// Executes a notebook cell in a session against the notebook's attached lakehouse.
/// Returns the cell output.
pub async fn run_exec(
    http: &Client,
    reference: &str,
    code: Option<&str>,
    cell_index: Option<usize>,
    lakehouse: Option<&str>,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    // Resolve the code to execute
    let exec_code = match (code, cell_index) {
        (Some(c), _) => c.to_string(),
        (None, Some(idx)) => {
            let def = client::get_definition(http, &ws_id, &nb.id, "ipynb").await?;
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

            let cells = ipynb
                .get("cells")
                .and_then(|c| c.as_array())
                .context("No cells array")?;

            let cell = cells
                .get(idx)
                .context(format!("Cell index {} out of range", idx))?;

            cell_source_string(cell)
        }
        (None, None) => anyhow::bail!(
            "Provide --code or a cell index.\n  nb exec cell \"Workspace/Notebook\" 0"
        ),
    };

    // Resolve lakehouse ID (from --lakehouse flag, or detect from notebook metadata)
    let lh_id = match lakehouse {
        Some(lh_name) => {
            let lh = client::resolve_item(http, &ws_id, lh_name, "Lakehouse").await?;
            lh.id
        }
        None => {
            detect_lakehouse(http, &ws_id, &nb.id).await
                .context("No lakehouse attached to notebook. Use --lakehouse <name> to specify one.")?
        }
    };

    // Detect kernel type
    let kernel = detect_kernel(http, &ws_id, &nb.id).await.unwrap_or_else(|_| "jupyter".to_string());
    let kind = if kernel == "synapse_pyspark" { "pyspark" } else { "python" };
    let session_name = format!("nb-cli-{}", nb.display_name);

    run_in_session(http, &ws_id, &lh_id, kind, &session_name, &exec_code).await
}


/// Shared session lifecycle: create session, wait, submit code, poll result, print, cleanup.
/// Always cleans up the session, even on errors after creation.
/// Returns a non-zero exit (Err) when the submitted code itself fails.
async fn run_in_session(
    http: &Client,
    ws_id: &str,
    lh_id: &str,
    kind: &str,
    session_name: &str,
    code: &str,
) -> Result<()> {
    let start = std::time::Instant::now();

    let sp = Spinner::start("Creating session...");

    // Create session
    let token = auth::get_fabric_token()?;
    let session_url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/lakehouses/{}/livyApi/versions/{}/sessions",
        ws_id, lh_id, LIVY_API_VERSION
    );

    let session_body = serde_json::json!({
        "kind": kind,
        "name": session_name
    });

    let resp = http
        .post(&session_url)
        .bearer_auth(&token)
        .json(&session_body)
        .send()
        .await
        .context("Failed to create session")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        sp.finish("Failed to create session", false).await;
        anyhow::bail!("Failed to create session ({}): {}", status, body);
    }

    let session: serde_json::Value = resp.json().await?;
    let session_id = session
        .get("id")
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .filter(|s| !s.is_empty())
        .context("No session ID returned")?;

    let short_id = session_id[..8].to_string();

    // From here on, always clean up the session -- even on Ctrl+C
    let result = tokio::select! {
        res = execute_and_print(http, ws_id, lh_id, &session_id, kind, code, sp) => res,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("{ERASE}  {RED}!!{RESET} {DIM}Interrupted{RESET}");
            Err(anyhow::anyhow!("Interrupted"))
        }
    };

    // Always clean up
    eprintln!();
    let _ = cleanup_session(http, ws_id, lh_id, &session_id).await;

    let elapsed = start.elapsed();
    let ok = result.is_ok();
    let status_str = if ok { format!("{GREEN}\u{2714}{RESET}") } else { format!("{RED}\u{2718}{RESET}") };
    eprintln!("  {DIM}---{RESET}");
    eprintln!("  {DIM}session{RESET}  {short_id}  {DIM}status{RESET}  {status_str}  {DIM}duration{RESET}  {:.1}s", elapsed.as_secs_f64());

    result
}


/// Wait for session, submit code, poll result, and print output.
/// Separated from run_in_session so cleanup always runs via the caller.
async fn execute_and_print(
    http: &Client,
    ws_id: &str,
    lh_id: &str,
    session_id: &str,
    kind: &str,
    code: &str,
    sp: Spinner,
) -> Result<()> {
    // Wait for session to be ready
    wait_for_session_idle(http, ws_id, lh_id, session_id, &sp).await?;

    sp.set_message("Submitting code...");

    // Submit statement
    let stmt_url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/lakehouses/{}/livyApi/versions/{}/sessions/{}/statements",
        ws_id, lh_id, LIVY_API_VERSION, session_id
    );

    let stmt_body = serde_json::json!({
        "code": code,
        "kind": kind
    });

    let token = auth::get_fabric_token()?;
    let stmt_resp = http
        .post(&stmt_url)
        .bearer_auth(&token)
        .json(&stmt_body)
        .send()
        .await
        .context("Failed to submit statement")?;

    let stmt: serde_json::Value = stmt_resp.json().await?;
    let stmt_id = stmt
        .get("id")
        .and_then(|v| v.as_u64())
        .context("No statement ID returned")?;

    // Poll for statement result
    let result = poll_statement(http, ws_id, lh_id, session_id, stmt_id, &sp).await?;

    // Print output
    let output_status = result
        .pointer("/output/status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if output_status == "ok" {
        sp.finish("Done", true).await;
        eprintln!();
        if let Some(text) = result.pointer("/output/data/text~1plain").and_then(|v| v.as_str()) {
            println!("{}", text);
        }
        Ok(())
    } else {
        let ename = result.pointer("/output/ename").and_then(|v| v.as_str()).unwrap_or("");
        let evalue = result.pointer("/output/evalue").and_then(|v| v.as_str()).unwrap_or("");
        sp.finish(&format!("{ename}: {evalue}"), false).await;
        if let Some(traceback) = result.pointer("/output/traceback").and_then(|v| v.as_array()) {
            for line in traceback {
                if let Some(s) = line.as_str() {
                    eprintln!("  {DIM}{s}{RESET}");
                }
            }
        }
        anyhow::bail!("Code execution failed: {} {}", ename, evalue)
    }
}


/// Clean up a session by sending a DELETE request.
/// Errors are silently ignored; this is best-effort cleanup.
async fn cleanup_session(
    http: &Client,
    ws_id: &str,
    lh_id: &str,
    session_id: &str,
) -> Result<()> {
    let token = auth::get_fabric_token()?;
    let url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/lakehouses/{}/livyApi/versions/{}/sessions/{}",
        ws_id, lh_id, LIVY_API_VERSION, session_id
    );
    let _ = http.delete(&url).bearer_auth(&token).send().await;
    eprintln!("  {GREEN}\u{2714}{RESET} {DIM}Session cleaned up{RESET}");
    Ok(())
}


/// Wait for a session to reach 'idle' state. Updates the spinner message.
async fn wait_for_session_idle(
    http: &Client,
    ws_id: &str,
    lh_id: &str,
    session_id: &str,
    sp: &Spinner,
) -> Result<()> {
    let check_url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/lakehouses/{}/livyApi/versions/{}/sessions/{}",
        ws_id, lh_id, LIVY_API_VERSION, session_id
    );

    for _ in 0..120 {
        let token = auth::get_fabric_token()?;
        let resp = http
            .get(&check_url)
            .bearer_auth(&token)
            .send()
            .await?;

        let session: serde_json::Value = resp.json().await?;
        let state = session.get("state").and_then(|v| v.as_str()).unwrap_or("unknown");

        match state {
            "idle" => return Ok(()),
            "dead" | "error" | "killed" => {
                anyhow::bail!("Session entered {} state", state);
            }
            _ => {
                sp.set_message("Starting session...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    anyhow::bail!("Session did not become idle within 10 minutes")
}


/// Poll a statement until it's available. Updates the spinner message.
async fn poll_statement(
    http: &Client,
    ws_id: &str,
    lh_id: &str,
    session_id: &str,
    stmt_id: u64,
    sp: &Spinner,
) -> Result<serde_json::Value> {
    let stmt_url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/lakehouses/{}/livyApi/versions/{}/sessions/{}/statements/{}",
        ws_id, lh_id, LIVY_API_VERSION, session_id, stmt_id
    );

    for _ in 0..120 {
        let token = auth::get_fabric_token()?;
        let resp = http
            .get(&stmt_url)
            .bearer_auth(&token)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let state = result.get("state").and_then(|v| v.as_str()).unwrap_or("unknown");

        match state {
            "available" => return Ok(result),
            "waiting" | "running" => {
                sp.set_message("Running...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            "error" | "cancelled" | "cancelling" => {
                return Ok(result);
            }
            _ => {
                sp.set_message("Running...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }

    anyhow::bail!("Statement did not complete within timeout")
}


/// Detect kernel name from notebook metadata.
async fn detect_kernel(http: &Client, ws_id: &str, item_id: &str) -> Result<String> {
    let def = client::get_definition(http, ws_id, item_id, "ipynb").await?;
    let parts = def.definition.context("No definition")?.parts;
    let part = parts
        .iter()
        .find(|p| p.path.contains("notebook-content") || p.path.ends_with(".ipynb"))
        .or_else(|| parts.first())
        .context("No parts")?;

    let decoded = BASE64.decode(&part.payload)?;
    let ipynb: serde_json::Value = serde_json::from_slice(&decoded)?;

    let kernel = ipynb
        .pointer("/metadata/kernel_info/name")
        .or_else(|| ipynb.pointer("/metadata/kernelspec/name"))
        .and_then(|v| v.as_str())
        .unwrap_or("jupyter")
        .to_string();

    Ok(kernel)
}


/// Detect the default lakehouse ID from notebook metadata.
async fn detect_lakehouse(http: &Client, ws_id: &str, item_id: &str) -> Result<String> {
    let def = client::get_definition(http, ws_id, item_id, "ipynb").await?;
    let parts = def.definition.context("No definition")?.parts;
    let part = parts
        .iter()
        .find(|p| p.path.contains("notebook-content") || p.path.ends_with(".ipynb"))
        .or_else(|| parts.first())
        .context("No parts")?;

    let decoded = BASE64.decode(&part.payload)?;
    let ipynb: serde_json::Value = serde_json::from_slice(&decoded)?;

    ipynb
        .pointer("/metadata/dependencies/lakehouse/default_lakehouse")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .context("No default_lakehouse in notebook metadata")
}


// #endregion
