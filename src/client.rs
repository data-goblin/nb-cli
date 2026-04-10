// #region Imports
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::auth;
// #endregion

// #region Variables
pub const FABRIC_BASE: &str = "https://api.fabric.microsoft.com/v1";
// #endregion

// #region Classes
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceList {
    value: Vec<Workspace>,
    #[serde(default)]
    continuation_uri: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FabricItem {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemList {
    value: Vec<FabricItem>,
    #[serde(default)]
    continuation_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDefinition {
    pub definition: Option<DefinitionBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionBody {
    pub parts: Vec<DefinitionPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionPart {
    pub path: String,
    pub payload: String,
    pub payload_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInstance {
    pub id: Option<String>,
    pub status: Option<String>,
    pub start_time_utc: Option<String>,
    pub end_time_utc: Option<String>,
    pub failure_reason: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobInstanceList {
    value: Vec<JobInstance>,
}
// #endregion

// #region Functions

/// Build a reqwest client with rustls.
pub fn build_client() -> Result<Client> {
    Client::builder()
        .use_rustls_tls()
        .build()
        .context("Failed to build HTTP client")
}


/// Resolve a workspace identifier (GUID or display name) to a workspace ID.
/// Strips a trailing ".Workspace" suffix if present.
/// Accepts a raw GUID or searches by displayName.
pub async fn resolve_workspace(client: &Client, name_or_id: &str) -> Result<String> {
    let cleaned = name_or_id
        .trim()
        .trim_end_matches(".Workspace");

    // If it looks like a GUID, return directly
    if is_guid(cleaned) {
        return Ok(cleaned.to_string());
    }

    let mut url = format!("{}/workspaces", FABRIC_BASE);
    loop {
        let token = auth::get_fabric_token()?;
        let resp = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to list workspaces")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET workspaces failed ({}): {}", status, body);
        }

        let list: WorkspaceList = resp.json().await?;
        for ws in &list.value {
            if ws.display_name.eq_ignore_ascii_case(cleaned) {
                return Ok(ws.id.clone());
            }
        }

        if let Some(next) = list.continuation_uri {
            url = next;
        } else {
            break;
        }
    }

    anyhow::bail!("Workspace '{}' not found", cleaned)
}


/// List items of a given type in a workspace.
pub async fn list_items(
    client: &Client,
    workspace_id: &str,
    item_type: &str,
) -> Result<Vec<FabricItem>> {
    let mut url = format!(
        "{}/workspaces/{}/items?type={}",
        FABRIC_BASE, workspace_id, item_type
    );
    let mut items = Vec::new();

    loop {
        let token = auth::get_fabric_token()?;
        let resp = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to list items")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET items failed ({}): {}", status, body);
        }

        let list: ItemList = resp.json().await?;
        items.extend(list.value);

        if let Some(next) = list.continuation_uri {
            url = next;
        } else {
            break;
        }
    }

    Ok(items)
}


/// Resolve an item by display name within a workspace.
pub async fn resolve_item(
    client: &Client,
    workspace_id: &str,
    item_name: &str,
    item_type: &str,
) -> Result<FabricItem> {
    let items = list_items(client, workspace_id, item_type).await?;
    items
        .into_iter()
        .find(|i| i.display_name.eq_ignore_ascii_case(item_name))
        .ok_or_else(|| anyhow::anyhow!("{} '{}' not found in workspace", item_type, item_name))
}


/// Parse a "workspace/item" reference into (workspace, item_name).
pub fn parse_ref(reference: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = reference.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        anyhow::bail!(
            "Invalid reference '{}'; expected format: workspace/item",
            reference
        );
    }
    Ok((parts[0], parts[1]))
}


/// Create an item via the Fabric REST API.
pub async fn create_item(
    client: &Client,
    workspace_id: &str,
    body: &serde_json::Value,
) -> Result<FabricItem> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .post(format!("{}/workspaces/{}/items", FABRIC_BASE, workspace_id))
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .context("Failed to create item")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("POST create item failed ({}): {}", status, body);
    }

    let item: FabricItem = resp.json().await?;
    Ok(item)
}


/// Get item definition (ipynb export).
/// Handles both 200 (immediate) and 202 (long-running operation) responses.
pub async fn get_definition(
    client: &Client,
    workspace_id: &str,
    item_id: &str,
    format: &str,
) -> Result<ItemDefinition> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .post(format!(
            "{}/workspaces/{}/items/{}/getDefinition?format={}",
            FABRIC_BASE, workspace_id, item_id, format
        ))
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .await
        .context("Failed to get item definition")?;

    let status = resp.status();

    if status.as_u16() == 200 {
        let def: ItemDefinition = resp.json().await?;
        return Ok(def);
    }

    if status.as_u16() == 202 {
        // Long-running operation; poll Location header
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2);

        if let Some(poll_url) = location {
            return poll_definition(client, &poll_url, retry_after).await;
        }
        anyhow::bail!("202 response but no Location header for polling");
    }

    let body = resp.text().await.unwrap_or_default();
    anyhow::bail!("getDefinition failed ({}): {}", status, body)
}


/// Poll a long-running operation until it completes and returns the definition.
async fn poll_definition(
    client: &Client,
    poll_url: &str,
    initial_retry: u64,
) -> Result<ItemDefinition> {
    let mut wait = initial_retry;

    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;

        let token = auth::get_fabric_token()?;
        let resp = client
            .get(poll_url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to poll operation")?;

        let status = resp.status();

        if status.as_u16() == 200 {
            let body: serde_json::Value = resp.json().await?;

            // Check if operation succeeded
            let op_status = body.pointer("/status").and_then(|v| v.as_str()).unwrap_or("");
            if op_status == "Succeeded" {
                // Fetch result from {pollUrl}/result
                let result_url = format!("{}/result", poll_url);
                let token = auth::get_fabric_token()?;
                let result_resp = client
                    .get(&result_url)
                    .bearer_auth(&token)
                    .send()
                    .await
                    .context("Failed to fetch operation result")?;
                let def: ItemDefinition = result_resp.json().await?;
                return Ok(def);
            }

            // Check if operation failed
            if op_status == "Failed" {
                let error = body.pointer("/error").cloned().unwrap_or(serde_json::Value::Null);
                anyhow::bail!("Operation failed: {}", error);
            }

            // Check for resourceLocation
            if let Some(result_url) = body.pointer("/resourceLocation").and_then(|v| v.as_str()) {
                let token = auth::get_fabric_token()?;
                let result_resp = client
                    .get(result_url)
                    .bearer_auth(&token)
                    .send()
                    .await?;
                let def: ItemDefinition = result_resp.json().await?;
                return Ok(def);
            }

            // Maybe the body itself is the definition
            if body.get("definition").is_some() {
                let def: ItemDefinition = serde_json::from_value(body)?;
                return Ok(def);
            }
        }

        if status.as_u16() == 202 {
            wait = 2;
            continue;
        }

        anyhow::bail!("Poll failed with status {}", status);
    }

    anyhow::bail!("Operation timed out after polling")
}


/// Run a notebook job.
pub async fn run_job(
    client: &Client,
    workspace_id: &str,
    item_id: &str,
) -> Result<String> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .post(format!(
            "{}/workspaces/{}/items/{}/jobs/instances?jobType=RunNotebook",
            FABRIC_BASE, workspace_id, item_id
        ))
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .await
        .context("Failed to run notebook job")?;

    let status = resp.status();

    // 202 is the expected response for job creation
    if status.as_u16() == 202 {
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        return Ok(location);
    }

    let body = resp.text().await.unwrap_or_default();
    anyhow::bail!("Run notebook failed ({}): {}", status, body)
}


/// List job instances for an item.
pub async fn list_job_instances(
    client: &Client,
    workspace_id: &str,
    item_id: &str,
) -> Result<Vec<JobInstance>> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .get(format!(
            "{}/workspaces/{}/items/{}/jobs/instances",
            FABRIC_BASE, workspace_id, item_id
        ))
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to list job instances")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET job instances failed ({}): {}", status, body);
    }

    let list: JobInstanceList = resp.json().await?;
    Ok(list.value)
}


/// Poll a job until it reaches a terminal state.
pub async fn poll_job(
    client: &Client,
    poll_url: &str,
    timeout_secs: u64,
) -> Result<JobInstance> {
    let start = std::time::Instant::now();

    loop {
        if start.elapsed().as_secs() > timeout_secs {
            anyhow::bail!("Job timed out after {}s", timeout_secs);
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let token = auth::get_fabric_token()?;
        let resp = client
            .get(poll_url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to poll job status")?;

        let status = resp.status();
        if status.as_u16() == 200 {
            let job: JobInstance = resp.json().await?;
            if let Some(ref s) = job.status {
                let s_lower = s.to_lowercase();
                if s_lower == "completed" || s_lower == "failed" || s_lower == "cancelled" {
                    return Ok(job);
                }
            }
        } else if status.as_u16() != 202 {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Job poll failed ({}): {}", status, body);
        }
    }
}


/// Update a notebook definition by base64-encoding the ipynb JSON and POSTing it.
/// Handles 200 (immediate success) and 202 (long-running operation with polling).
pub async fn update_definition(
    client: &Client,
    workspace_id: &str,
    item_id: &str,
    ipynb_json: &serde_json::Value,
) -> Result<()> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    let token = auth::get_fabric_token()?;
    let ipynb_str = serde_json::to_string_pretty(ipynb_json)?;
    let encoded = BASE64.encode(ipynb_str.as_bytes());

    let body = serde_json::json!({
        "definition": {
            "format": "ipynb",
            "parts": [{
                "path": "notebook-content.ipynb",
                "payload": encoded,
                "payloadType": "InlineBase64"
            }]
        }
    });

    let resp = client
        .post(format!(
            "{}/workspaces/{}/items/{}/updateDefinition",
            FABRIC_BASE, workspace_id, item_id
        ))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .context("Failed to update definition")?;

    let status = resp.status();

    if status.as_u16() == 200 {
        return Ok(());
    }

    if status.as_u16() == 202 {
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2);

        if let Some(poll_url) = location {
            poll_until_done(client, &poll_url, retry_after).await?;
            return Ok(());
        }
        // No location header but 202; assume eventual success
        return Ok(());
    }

    let resp_body = resp.text().await.unwrap_or_default();
    anyhow::bail!("updateDefinition failed ({}): {}", status, resp_body)
}


/// Poll a long-running operation URL until it returns 200.
async fn poll_until_done(
    client: &Client,
    poll_url: &str,
    initial_retry: u64,
) -> Result<()> {
    let mut wait = initial_retry;

    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;

        let token = auth::get_fabric_token()?;
        let resp = client
            .get(poll_url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to poll operation")?;

        let status = resp.status();

        if status.as_u16() == 200 {
            return Ok(());
        }

        if status.as_u16() == 202 {
            wait = 2;
            continue;
        }

        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Poll failed ({}): {}", status, body);
    }

    anyhow::bail!("Operation timed out after polling")
}


/// Get the capacity ID for a workspace.
/// Fetches workspace details and extracts the capacityId field.
pub async fn get_workspace_capacity(
    client: &Client,
    workspace_id: &str,
) -> Result<String> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .get(format!("{}/workspaces/{}", FABRIC_BASE, workspace_id))
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to get workspace details")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET workspace failed ({}): {}", status, body);
    }

    let ws: serde_json::Value = resp.json().await?;
    let capacity_id = ws
        .get("capacityId")
        .and_then(|v| v.as_str())
        .context("No capacityId in workspace details")?
        .to_string();

    Ok(capacity_id)
}


/// Get the WABI host from the Fabric API response headers.
/// The `home-cluster-uri` header contains the region-specific WABI endpoint.
pub async fn get_wabi_host(client: &Client) -> Result<String> {
    let token = auth::get_fabric_token()?;
    let resp = client
        .get(format!("{}/workspaces", FABRIC_BASE))
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to call Fabric API for WABI host")?;

    let wabi = resp
        .headers()
        .get("home-cluster-uri")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "https://wabi-west-europe-e-primary-redirect.analysis.windows.net".to_string());

    Ok(wabi)
}


/// Check if a string looks like a GUID.
fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
        && s.matches('-').count() == 4
}

// #endregion
