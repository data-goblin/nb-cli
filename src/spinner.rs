// #region Imports
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;
// #endregion

// #region Variables
const FRAMES: &[&str] = &["\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280F}"];
const CHECK: &str = "\u{2714}";
const CROSS: &str = "\u{2718}";

static QUIET: AtomicBool = AtomicBool::new(false);
// #endregion

// #region Quiet mode

/// Enable quiet mode: spinners become no-ops and ANSI styles render empty.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}


/// Whether quiet mode is active.
pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}
// #endregion

// #region Style

/// ANSI style wrapper that renders as the empty string when quiet mode is on.
/// Preserves the `{DIM}`-in-format-string ergonomics of the original constants
/// while making the whole CLI script-friendly when `-q` is passed.
#[derive(Copy, Clone)]
pub struct Style(pub &'static str);

impl std::fmt::Display for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if is_quiet() {
            Ok(())
        } else {
            f.write_str(self.0)
        }
    }
}

pub const CYAN: Style = Style("\x1b[36m");
pub const DIM: Style = Style("\x1b[2m");
pub const GREEN: Style = Style("\x1b[32m");
pub const RED: Style = Style("\x1b[31m");
pub const RESET: Style = Style("\x1b[0m");
pub const ERASE: Style = Style("\x1b[2K\r");
// #endregion

// #region Spinner

/// Background spinner that animates at ~80ms on stderr.
/// Update the message with `set_message()`, stop with `finish()`.
/// In quiet mode the spinner is a no-op that does not spawn a task or emit output.
pub struct Spinner {
    msg: Arc<std::sync::Mutex<String>>,
    stop: Arc<Notify>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(initial_msg: &str) -> Self {
        let msg = Arc::new(std::sync::Mutex::new(initial_msg.to_string()));
        let stop = Arc::new(Notify::new());

        if is_quiet() {
            return Self { msg, stop, handle: None };
        }

        let msg_clone = Arc::clone(&msg);
        let stop_clone = Arc::clone(&stop);

        let handle = tokio::spawn(async move {
            let mut frame: usize = 0;
            loop {
                let current = msg_clone.lock().unwrap().clone();
                let ch = FRAMES[frame % FRAMES.len()];
                eprint!("{ERASE}  {CYAN}{ch}{RESET} {DIM}{current}{RESET}");
                let _ = std::io::stderr().flush();
                frame += 1;

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(80)) => {}
                    _ = stop_clone.notified() => break,
                }
            }
        });

        Self { msg, stop, handle: Some(handle) }
    }

    pub fn set_message(&self, new_msg: &str) {
        *self.msg.lock().unwrap() = new_msg.to_string();
    }

    pub async fn finish(self, msg: &str, ok: bool) {
        self.stop.notify_one();
        if let Some(handle) = self.handle {
            let _ = handle.await;
        }
        if is_quiet() {
            if ok {
                eprintln!("  {}", msg);
            } else {
                eprintln!("  [failed] {}", msg);
            }
            return;
        }
        let icon = if ok { format!("{GREEN}{CHECK}{RESET}") } else { format!("{RED}{CROSS}{RESET}") };
        eprintln!("{ERASE}  {icon} {DIM}{msg}{RESET}");
    }
}

// #endregion
