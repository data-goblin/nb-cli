// #region Imports
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
// #endregion

// #region Variables
static TOKEN_CACHE: Mutex<Option<CachedToken>> = Mutex::new(None);

const FABRIC_RESOURCE: &str = "https://analysis.windows.net/powerbi/api";
// #endregion

// #region Classes
#[derive(Debug, Deserialize, Clone)]
pub struct AzToken {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    /// Legacy local-time date string from az (e.g. "2026-04-13 16:30:18.000000").
    /// Kept for display only; never use for expiry math.
    #[serde(rename = "expiresOn")]
    pub expires_on: String,
    /// Unix epoch seconds; the canonical expiry from modern az CLI.
    #[serde(rename = "expires_on", default)]
    pub expires_on_epoch: Option<i64>,
    pub tenant: Option<String>,
    pub subscription: Option<String>,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: u64,
}

#[derive(Debug)]
pub struct AuthStatus {
    pub tenant: String,
    pub subscription: String,
    pub token_valid: bool,
    pub expires_on: String,
}
// #endregion

// #region Functions

/// Resolve the full path to the Azure CLI executable.
/// On Windows, `az` ships as `az.cmd`, which `Command::new("az")` cannot find
/// because `CreateProcessW` ignores `PATHEXT`. The `which` crate honours
/// `PATHEXT` and returns the first match (e.g. `az.cmd`). Result is cached.
fn az_executable() -> Result<PathBuf> {
    static AZ_PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = AZ_PATH.get() {
        return Ok(path.clone());
    }
    let path = which::which("az")
        .context("Failed to locate 'az' CLI on PATH; is Azure CLI installed?")?;
    let _ = AZ_PATH.set(path.clone());
    Ok(path)
}


/// Acquire a token from Azure CLI for the given resource.
/// Shells out to `az account get-access-token` and parses the JSON response.
/// Returns an error if `az` is not installed or not authenticated.
fn az_get_token(resource: &str) -> Result<AzToken> {
    let az = az_executable()?;
    let output = Command::new(&az)
        .args([
            "account",
            "get-access-token",
            "--resource",
            resource,
            "--output",
            "json",
        ])
        .output()
        .context("Failed to execute az CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("az account get-access-token failed: {}", stderr.trim());
    }

    let token: AzToken =
        serde_json::from_slice(&output.stdout).context("Failed to parse az CLI token output")?;
    Ok(token)
}


/// Resolve token expiry to a Unix epoch in seconds.
/// Uses the canonical integer `expires_on` from az CLI. Returns 0 if absent,
/// which forces a refresh on the next call (correct, just non-cached).
fn token_expiry_epoch(token: &AzToken) -> u64 {
    token.expires_on_epoch.filter(|&e| e > 0).unwrap_or(0) as u64
}


/// Get the current Unix timestamp in seconds.
fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}


/// Get a cached or fresh Fabric API token.
/// Checks expiry with a 60-second buffer before refreshing.
pub fn get_fabric_token() -> Result<String> {
    get_cached_token(&TOKEN_CACHE, FABRIC_RESOURCE)
}


/// Internal: check cache, refresh if expired, return token string.
fn get_cached_token(cache: &Mutex<Option<CachedToken>>, resource: &str) -> Result<String> {
    let mut lock = cache.lock().unwrap();
    if let Some(ref cached) = *lock {
        if now_epoch() + 60 < cached.expires_at {
            return Ok(cached.token.clone());
        }
    }

    let az = az_get_token(resource)?;
    let expires_at = token_expiry_epoch(&az);
    let token = az.access_token.clone();
    *lock = Some(CachedToken {
        token: token.clone(),
        expires_at,
    });
    Ok(token)
}


/// Check auth status without caching; returns tenant/subscription info.
pub fn check_auth_status() -> Result<AuthStatus> {
    let az = az_get_token(FABRIC_RESOURCE)?;
    let expiry = token_expiry_epoch(&az);
    Ok(AuthStatus {
        tenant: az.tenant.unwrap_or_else(|| "unknown".into()),
        subscription: az.subscription.unwrap_or_else(|| "unknown".into()),
        token_valid: expiry > now_epoch(),
        expires_on: az.expires_on,
    })
}

// #endregion
