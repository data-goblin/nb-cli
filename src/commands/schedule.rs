// #region Imports
use anyhow::{Context, Result};
use reqwest::Client;

use crate::auth;
use crate::client;
use crate::client::FABRIC_BASE;
// #endregion

// #region Functions

/// Handle `nb schedule list <workspace/notebook>` command.
pub async fn run_schedule_list(http: &Client, reference: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let token = auth::get_fabric_token()?;
    let url = format!(
        "{}/workspaces/{}/items/{}/jobs/RunNotebook/schedules",
        FABRIC_BASE, ws_id, nb.id
    );

    let resp = http
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to list schedules")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET schedules failed ({}): {}", status, body);
    }

    let body: serde_json::Value = resp.json().await?;
    let schedules = body.get("value").and_then(|v| v.as_array());

    match schedules {
        Some(arr) if !arr.is_empty() => {
            println!(
                "  {:<38}  {:<8}  {:<10}  {:<8}  {}",
                "ID", "Enabled", "Type", "Interval", "Start"
            );
            println!(
                "  {:<38}  {:<8}  {:<10}  {:<8}  {}",
                "-".repeat(36), "-".repeat(7), "-".repeat(9), "-".repeat(7), "-".repeat(20)
            );
            for s in arr {
                let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                let enabled = s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                let config = s.get("configuration").unwrap_or(&serde_json::Value::Null);
                let stype = config.get("type").and_then(|v| v.as_str()).unwrap_or("-");
                let interval = config.get("interval").and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
                let start = config.get("startDateTime").and_then(|v| v.as_str()).unwrap_or("-");
                println!(
                    "  {:<38}  {:<8}  {:<10}  {:<8}  {}",
                    id,
                    if enabled { "yes" } else { "no" },
                    stype,
                    interval,
                    start
                );
            }
            println!("\n  {} schedule(s)", arr.len());
        }
        _ => {
            println!("  No schedules found");
        }
    }

    Ok(())
}


/// Handle `nb schedule create <workspace/notebook>` command.
pub async fn run_schedule_create(
    http: &Client,
    reference: &str,
    schedule_type: &str,
    interval: u64,
    start: &str,
    end: Option<&str>,
    timezone: &str,
    enabled: bool,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let token = auth::get_fabric_token()?;
    let url = format!(
        "{}/workspaces/{}/items/{}/jobs/RunNotebook/schedules",
        FABRIC_BASE, ws_id, nb.id
    );

    let mut config = serde_json::json!({
        "startDateTime": start,
        "localTimeZoneId": timezone,
        "type": schedule_type,
        "interval": interval
    });

    if let Some(e) = end {
        config["endDateTime"] = serde_json::Value::String(e.to_string());
    }

    let body = serde_json::json!({
        "enabled": enabled,
        "configuration": config
    });

    let resp = http
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .context("Failed to create schedule")?;

    let status = resp.status();
    if status.as_u16() == 201 || status.is_success() {
        let result: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        let id = result.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        println!("  Schedule created: {}", id);
        println!("  Type: {}, Interval: {}, Enabled: {}", schedule_type, interval, enabled);
    } else {
        let resp_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Create schedule failed ({}): {}", status, resp_body);
    }

    Ok(())
}


/// Handle `nb schedule update <workspace/notebook> <schedule-id>` command.
pub async fn run_schedule_update(
    http: &Client,
    reference: &str,
    schedule_id: &str,
    enabled: Option<bool>,
    interval: Option<u64>,
    schedule_type: Option<&str>,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let token = auth::get_fabric_token()?;
    let url = format!(
        "{}/workspaces/{}/items/{}/jobs/RunNotebook/schedules/{}",
        FABRIC_BASE, ws_id, nb.id, schedule_id
    );

    // Fabric's schedule PATCH requires the full `configuration` object even
    // for partial updates. Fetch the current schedule, then send back a
    // minimal allow-listed `{enabled, configuration}` body merged with any
    // user-supplied overrides. Allow-listing avoids echoing server-managed
    // fields (`id`, `createdDateTime`, `owner`, future additions) that
    // Fabric rejects on round-trip.
    let current = http
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to fetch current schedule")?;
    if !current.status().is_success() {
        let status = current.status();
        let body = current.text().await.unwrap_or_default();
        anyhow::bail!("GET schedule failed ({}): {}", status, body);
    }
    let current: serde_json::Value = current.json().await?;

    let mut config = current
        .get("configuration")
        .cloned()
        .context("Schedule response missing 'configuration' object")?;
    let mut body = serde_json::json!({
        "enabled": current.get("enabled").cloned().unwrap_or(serde_json::Value::Bool(true)),
    });

    if let Some(e) = enabled {
        body["enabled"] = serde_json::Value::Bool(e);
    }
    if let Some(i) = interval {
        config["interval"] = serde_json::json!(i);
    }
    if let Some(t) = schedule_type {
        config["type"] = serde_json::Value::String(t.to_string());
    }
    body["configuration"] = config;

    let resp = http
        .patch(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .context("Failed to update schedule")?;

    let status = resp.status();
    if status.is_success() {
        println!("  Schedule {} updated", schedule_id);
    } else {
        let resp_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Update schedule failed ({}): {}", status, resp_body);
    }

    Ok(())
}


/// Handle `nb schedule delete <workspace/notebook> <schedule-id>` command.
pub async fn run_schedule_delete(
    http: &Client,
    reference: &str,
    schedule_id: &str,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let token = auth::get_fabric_token()?;
    let url = format!(
        "{}/workspaces/{}/items/{}/jobs/RunNotebook/schedules/{}",
        FABRIC_BASE, ws_id, nb.id, schedule_id
    );

    let resp = http
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to delete schedule")?;

    let status = resp.status();
    if status.is_success() {
        println!("  Schedule {} deleted", schedule_id);
    } else {
        let resp_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Delete schedule failed ({}): {}", status, resp_body);
    }

    Ok(())
}

// #endregion
