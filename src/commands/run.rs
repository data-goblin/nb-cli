// #region Imports
use anyhow::Result;
use reqwest::Client;

use crate::client;
use crate::spinner::{Spinner, DIM, GREEN, RED, RESET};
// #endregion

// #region Functions

/// Handle `nb job run <workspace/notebook>` command.
/// Triggers a notebook run and optionally waits for completion.
pub async fn run_run(
    http: &Client,
    reference: &str,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let sp = Spinner::start(&format!("Submitting job for '{}'...", nb.display_name));
    let poll_url = client::run_job(http, &ws_id, &nb.id).await?;
    sp.finish(&format!("Job submitted for '{}'", nb.display_name), true).await;

    if wait {
        let sp = Spinner::start("Waiting for completion...");
        let result = client::poll_job(http, &poll_url, timeout).await?;
        let status = result.status.as_deref().unwrap_or("unknown");
        let ok = status.eq_ignore_ascii_case("completed");
        sp.finish(&format!("Job {}", status), ok).await;

        eprintln!();
        if let Some(ref start) = result.start_time_utc {
            eprintln!("  {DIM}started{RESET}   {start}");
        }
        if let Some(ref end) = result.end_time_utc {
            eprintln!("  {DIM}ended{RESET}     {end}");
        }
        if let Some(ref reason) = result.failure_reason {
            if !reason.is_null() {
                eprintln!("  {RED}failure{RESET}   {reason}");
            }
        }
    } else {
        eprintln!("  {DIM}poll_url{RESET}  {poll_url}");
        eprintln!("  {DIM}Use --wait to block until completion{RESET}");
    }

    Ok(())
}


/// Handle `nb job list <workspace/notebook>` command.
/// Lists recent job instances for a notebook.
pub async fn run_runs(http: &Client, reference: &str) -> Result<()> {
    let (ws_name, nb_name) = client::parse_ref(reference)?;
    let ws_id = client::resolve_workspace(http, ws_name).await?;
    let nb = client::resolve_item(http, &ws_id, nb_name, "Notebook").await?;

    let jobs = client::list_job_instances(http, &ws_id, &nb.id).await?;

    if jobs.is_empty() {
        eprintln!("  {DIM}No recent runs for '{}'{RESET}", nb.display_name);
        return Ok(());
    }

    println!(
        "  {:<36}  {:<12}  {:<24}  {}",
        "Job ID", "Status", "Start", "End"
    );
    println!(
        "  {:<36}  {:<12}  {:<24}  {}",
        "-".repeat(36),
        "-".repeat(12),
        "-".repeat(24),
        "-".repeat(24)
    );

    for job in &jobs {
        let status = job.status.as_deref().unwrap_or("-");
        let color = if status.eq_ignore_ascii_case("completed") { GREEN }
                    else if status.eq_ignore_ascii_case("failed") { RED }
                    else { DIM };
        println!(
            "  {:<36}  {color}{:<12}{RESET}  {:<24}  {}",
            job.id.as_deref().unwrap_or("-"),
            status,
            job.start_time_utc.as_deref().unwrap_or("-"),
            job.end_time_utc.as_deref().unwrap_or("-"),
        );
    }

    eprintln!("\n  {DIM}{} run(s){RESET}", jobs.len());
    Ok(())
}

// #endregion
