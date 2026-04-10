// #region Imports
mod auth;
mod client;
mod commands;
mod notebook;
pub mod spinner;

use anyhow::Result;
use clap::{Parser, Subcommand};
// #endregion

// #region Classes

#[derive(Parser)]
#[command(name = "nb", version, about = "Fabric notebook CLI [experimental; commands may change]")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authentication commands
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// List notebooks in a workspace
    #[command(after_long_help = "\
Examples:
  nb list \"My Workspace\"
  nb list 5feab05a-68a8-43af-a621-2571b5fcd3fd")]
    List {
        /// Workspace name or GUID
        workspace: String,
    },

    /// Create a new notebook
    #[command(after_long_help = "\
Examples:
  nb create \"My Workspace/ETL Pipeline\" --kernel python --lakehouse MainLH
  nb create \"My Workspace/Spark Job\" --kernel pyspark --lakehouse MainLH
  nb create \"My Workspace/Analytics\" --kernel python --warehouse MyWarehouse")]
    Create {
        /// Workspace/notebook-name reference
        reference: String,

        /// Kernel type
        #[arg(long, default_value = "python")]
        kernel: String,

        /// Default lakehouse name
        #[arg(long)]
        lakehouse: Option<String>,

        /// Default warehouse name
        #[arg(long)]
        warehouse: Option<String>,
    },

    /// Batch job management (run, list history)
    Job {
        #[command(subcommand)]
        action: JobAction,
    },

    /// Open notebook in browser
    Open {
        /// Workspace/notebook reference
        reference: String,
    },

    /// Export notebook to local .ipynb file
    #[command(after_long_help = "\
Examples:
  nb export \"My Workspace/ETL Pipeline\" -o ./notebooks/etl.ipynb")]
    Export {
        /// Workspace/notebook reference
        reference: String,

        /// Output file path
        #[arg(short, long)]
        output: String,
    },

    /// List all cells in a notebook
    #[command(after_long_help = "\
Examples:
  nb cells \"My Workspace/ETL Pipeline\"")]
    Cells {
        /// Workspace/notebook reference
        reference: String,
    },

    /// View, add, edit, or remove a cell
    Cell {
        #[command(subcommand)]
        action: CellAction,
    },

    /// Execute code interactively and return output
    #[command(after_long_help = "\
Subcommands:
  nb exec code  Run code directly against a lakehouse (no notebook needed)
  nb exec cell  Execute a notebook cell via its attached lakehouse

Examples:
  nb exec code \"MyWorkspace/MyLH.Lakehouse\" \"print('hello')\"
  nb exec code \"MyWorkspace/MyLH.Lakehouse\" \"spark.sql('SHOW TABLES').show()\"
  nb exec cell \"My Workspace/Notebook\" 0 --lakehouse MainLH")]
    Exec {
        #[command(subcommand)]
        action: ExecAction,
    },

    /// Show active notebook sessions
    Session {
        /// Workspace/notebook reference
        reference: String,
    },

    /// Manage notebook schedules
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },

    /// Delete a notebook
    #[command(after_long_help = "\
Examples:
  nb delete \"My Workspace/Old Notebook\" --force")]
    Delete {
        /// Workspace/notebook reference
        reference: String,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ExecAction {
    /// Run code directly against a lakehouse (no notebook needed)
    #[command(after_long_help = "\
Creates an ephemeral session, runs the code, prints output, and cleans up.
The spark (SparkSession) variable is available for querying lakehouse tables.
Pass - as code to read from stdin.

Examples:
  nb exec code \"MyWorkspace/MyLH.Lakehouse\" \"print('hello')\"
  nb exec code \"MyWorkspace/MyLH.Lakehouse\" \"spark.sql('SHOW TABLES').show()\"
  echo \"print(42)\" | nb exec code \"MyWorkspace/MyLH.Lakehouse\" -")]
    Code {
        /// Workspace/lakehouse reference (e.g. \"MyWS/MyLH.Lakehouse\")
        reference: String,

        /// Code to execute (use - for stdin)
        code: String,
    },

    /// Execute a notebook cell via its attached lakehouse
    #[command(after_long_help = "\
Auto-detects kernel and lakehouse from notebook metadata.

Examples:
  nb exec cell \"My Workspace/Notebook\" 0
  nb exec cell \"My Workspace/Notebook\" 0 --lakehouse MainLH")]
    Cell {
        /// Workspace/notebook reference
        reference: String,

        /// Cell index to execute
        index: usize,

        /// Lakehouse name (auto-detected from notebook metadata if not specified)
        #[arg(long)]
        lakehouse: Option<String>,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Check authentication status
    Status,
}

#[derive(Subcommand)]
enum JobAction {
    /// Run a notebook as a batch job
    #[command(after_long_help = "\
Examples:
  nb job run \"My Workspace/ETL Pipeline\"
  nb job run \"My Workspace/ETL Pipeline\" --wait
  nb job run \"My Workspace/ETL Pipeline\" --wait --timeout 600")]
    Run {
        /// Workspace/notebook reference
        reference: String,

        /// Wait for completion
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds (with --wait)
        #[arg(long, default_value = "3600")]
        timeout: u64,
    },

    /// List recent job runs
    #[command(after_long_help = "\
Examples:
  nb job list \"My Workspace/ETL Pipeline\"")]
    List {
        /// Workspace/notebook reference
        reference: String,
    },
}

#[derive(Subcommand)]
enum CellAction {
    /// View a single cell
    View {
        /// Workspace/notebook reference
        reference: String,
        /// Cell index
        index: usize,
    },

    /// Add a new cell
    Add {
        /// Workspace/notebook reference
        reference: String,

        /// Code or markdown content
        #[arg(long)]
        code: String,

        /// Create a markdown cell instead of code
        #[arg(long)]
        markdown: bool,

        /// Insert at position (default: append)
        #[arg(long)]
        at: Option<usize>,
    },

    /// Edit a cell's source
    Edit {
        /// Workspace/notebook reference
        reference: String,

        /// Cell index
        index: usize,

        /// New source code
        #[arg(long)]
        code: String,
    },

    /// Remove a cell
    Rm {
        /// Workspace/notebook reference
        reference: String,

        /// Cell index
        index: usize,
    },
}

#[derive(Subcommand)]
enum ScheduleAction {
    /// List schedules for a notebook
    List {
        /// Workspace/notebook reference
        reference: String,
    },

    /// Create a schedule
    Create {
        /// Workspace/notebook reference
        reference: String,

        /// Schedule type: Cron, Daily, Weekly
        #[arg(long, default_value = "Cron")]
        r#type: String,

        /// Interval (minutes for Cron, times per day for Daily)
        #[arg(long)]
        interval: u64,

        /// Start datetime (ISO 8601)
        #[arg(long)]
        start: String,

        /// End datetime (ISO 8601)
        #[arg(long)]
        end: Option<String>,

        /// Timezone
        #[arg(long, default_value = "UTC")]
        timezone: String,

        /// Enable immediately
        #[arg(long)]
        enable: bool,
    },

    /// Update an existing schedule
    Update {
        /// Workspace/notebook reference
        reference: String,

        /// Schedule ID to update
        id: String,

        /// Enable or disable
        #[arg(long)]
        enable: Option<bool>,

        /// New interval
        #[arg(long)]
        interval: Option<u64>,

        /// New schedule type
        #[arg(long)]
        r#type: Option<String>,
    },

    /// Delete a schedule
    Delete {
        /// Workspace/notebook reference
        reference: String,

        /// Schedule ID to delete
        id: String,
    },
}

// #endregion

// #region Functions

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let http = client::build_client()?;

    match cli.command {
        Commands::Auth { action } => match action {
            AuthAction::Status => commands::auth::run_auth_status()?,
        },

        Commands::List { workspace } => {
            commands::list::run_list(&http, &workspace).await?;
        }

        Commands::Create {
            reference,
            kernel,
            lakehouse,
            warehouse,
        } => {
            commands::create::run_create(
                &http,
                &reference,
                &kernel,
                lakehouse.as_deref(),
                warehouse.as_deref(),
            )
            .await?;
        }

        Commands::Job { action } => match action {
            JobAction::Run {
                reference,
                wait,
                timeout,
            } => {
                commands::run::run_run(&http, &reference, wait, timeout).await?;
            }
            JobAction::List { reference } => {
                commands::run::run_runs(&http, &reference).await?;
            }
        },

        Commands::Open { reference } => {
            commands::open::run_open(&http, &reference).await?;
        }

        Commands::Export { reference, output } => {
            commands::export::run_export(&http, &reference, &output).await?;
        }

        Commands::Cells { reference } => {
            commands::cells::run_cells_list(&http, &reference).await?;
        }

        Commands::Cell { action } => match action {
            CellAction::View { reference, index } => {
                commands::cells::run_cell_view(&http, &reference, index).await?;
            }
            CellAction::Add {
                reference,
                code,
                markdown,
                at,
            } => {
                commands::cells::run_cell_add(&http, &reference, &code, markdown, at).await?;
            }
            CellAction::Edit {
                reference,
                index,
                code,
            } => {
                commands::cells::run_cell_edit(&http, &reference, index, &code).await?;
            }
            CellAction::Rm { reference, index } => {
                commands::cells::run_cell_rm(&http, &reference, index).await?;
            }
        },

        Commands::Exec { action } => match action {
            ExecAction::Code {
                reference,
                code,
            } => {
                commands::exec::run_exec_quick(&http, &reference, &code).await?;
            }
            ExecAction::Cell {
                reference,
                index,
                lakehouse,
            } => {
                commands::exec::run_exec(&http, &reference, None, Some(index), lakehouse.as_deref()).await?;
            }
        },

        Commands::Session { reference } => {
            commands::session::run_session(&http, &reference).await?;
        }

        Commands::Schedule { action } => match action {
            ScheduleAction::List { reference } => {
                commands::schedule::run_schedule_list(&http, &reference).await?;
            }
            ScheduleAction::Create {
                reference,
                r#type,
                interval,
                start,
                end,
                timezone,
                enable,
            } => {
                commands::schedule::run_schedule_create(
                    &http,
                    &reference,
                    &r#type,
                    interval,
                    &start,
                    end.as_deref(),
                    &timezone,
                    enable,
                )
                .await?;
            }
            ScheduleAction::Update {
                reference,
                id,
                enable,
                interval,
                r#type,
            } => {
                commands::schedule::run_schedule_update(
                    &http,
                    &reference,
                    &id,
                    enable,
                    interval,
                    r#type.as_deref(),
                )
                .await?;
            }
            ScheduleAction::Delete { reference, id } => {
                commands::schedule::run_schedule_delete(&http, &reference, &id).await?;
            }
        },

        Commands::Delete { reference, force } => {
            let (ws_name, nb_name) = client::parse_ref(&reference)?;
            let ws_id = client::resolve_workspace(&http, ws_name).await?;
            let nb = client::resolve_item(&http, &ws_id, nb_name, "Notebook").await?;

            if !force {
                anyhow::bail!("Delete '{}' requires --force to confirm", nb.display_name);
            } else {
                let token = auth::get_fabric_token()?;
                let resp = http
                    .delete(format!(
                        "https://api.fabric.microsoft.com/v1/workspaces/{}/items/{}",
                        ws_id, nb.id
                    ))
                    .bearer_auth(&token)
                    .send()
                    .await?;

                if resp.status().is_success() {
                    println!("  Deleted '{}'", nb.display_name);
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    anyhow::bail!("Delete failed: {}", body);
                }
            }
        }
    }

    Ok(())
}

// #endregion
