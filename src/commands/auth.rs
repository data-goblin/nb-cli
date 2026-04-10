// #region Imports
use anyhow::Result;

use crate::auth as auth_mod;
// #endregion

// #region Functions

/// Handle `nb auth status` command.
/// Checks Azure CLI authentication and displays tenant/subscription info.
pub fn run_auth_status() -> Result<()> {
    let status = auth_mod::check_auth_status()?;

    let valid_str = if status.token_valid { "valid" } else { "EXPIRED" };

    println!("  Azure CLI Authentication");
    println!("  {}", "-".repeat(40));
    println!("  Tenant         {}", status.tenant);
    println!("  Subscription   {}", status.subscription);
    println!("  Token          {}", valid_str);
    println!("  Expires        {}", status.expires_on);

    if !status.token_valid {
        eprintln!("\n  Token is expired. Run: az login");
    }

    Ok(())
}

// #endregion
