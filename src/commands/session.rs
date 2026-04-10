// #region Imports
use anyhow::{Context, Result};
use reqwest::Client;

use crate::auth;
use crate::client;
// #endregion

// #region Classes
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LivySession {
    id: Option<i64>,
    state: Option<String>,
    app_id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LivySessionList {
    #[serde(default)]
    value: Vec<LivySession>,
}
// #endregion

// #region Functions

/// Handle `nb session <workspace/notebook>` command.
/// Shows active Livy sessions for the notebook.
pub async fn run_session(http: &Client, reference: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let token = auth::get_fabric_token()?;
    let url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/notebooks/{}/livySessions",
        ws_id, nb.id
    );

    let resp = http
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to fetch Livy sessions")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET livySessions failed ({}): {}", status, body);
    }

    let list: LivySessionList = resp.json().await.context("Failed to parse sessions")?;

    if list.value.is_empty() {
        println!("  No active sessions for '{}'", nb.display_name);
        return Ok(());
    }

    println!("  {:<8}  {:<12}  {:<24}  {}", "ID", "State", "App ID", "Name");
    println!(
        "  {:<8}  {:<12}  {:<24}  {}",
        "--------", "------------", "------------------------", "----"
    );

    for s in &list.value {
        println!(
            "  {:<8}  {:<12}  {:<24}  {}",
            s.id.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
            s.state.as_deref().unwrap_or("-"),
            s.app_id.as_deref().unwrap_or("-"),
            s.name.as_deref().unwrap_or("-"),
        );
    }

    println!("\n  {} session(s)", list.value.len());
    Ok(())
}

// #endregion
