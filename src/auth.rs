// #region Imports
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
// #endregion

// #region Variables
static TOKEN_CACHE: Mutex<Option<CachedToken>> = Mutex::new(None);
static STORAGE_TOKEN_CACHE: Mutex<Option<CachedToken>> = Mutex::new(None);

const FABRIC_RESOURCE: &str = "https://analysis.windows.net/powerbi/api";
const STORAGE_RESOURCE: &str = "https://storage.azure.com";
// #endregion

// #region Classes
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AzToken {
    pub access_token: String,
    pub expires_on: String,
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

/// Acquire a token from Azure CLI for the given resource.
/// Shells out to `az account get-access-token` and parses the JSON response.
/// Returns an error if `az` is not installed or not authenticated.
fn az_get_token(resource: &str) -> Result<AzToken> {
    let output = Command::new("az")
        .args([
            "account",
            "get-access-token",
            "--resource",
            resource,
            "--output",
            "json",
        ])
        .output()
        .context("Failed to run 'az' CLI; is Azure CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("az account get-access-token failed: {}", stderr.trim());
    }

    let token: AzToken =
        serde_json::from_slice(&output.stdout).context("Failed to parse az CLI token output")?;
    Ok(token)
}


/// Parse the expires_on field (Unix timestamp string) into a u64.
fn parse_expires_on(expires_on: &str) -> u64 {
    expires_on.trim().parse::<u64>().unwrap_or(0)
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


/// Get a cached or fresh OneLake storage token.
pub fn get_storage_token() -> Result<String> {
    get_cached_token(&STORAGE_TOKEN_CACHE, STORAGE_RESOURCE)
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
    let expires_at = parse_expires_on(&az.expires_on);
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
    Ok(AuthStatus {
        tenant: az.tenant.unwrap_or_else(|| "unknown".into()),
        subscription: az.subscription.unwrap_or_else(|| "unknown".into()),
        token_valid: parse_expires_on(&az.expires_on) > now_epoch(),
        expires_on: az.expires_on,
    })
}

// #endregion
