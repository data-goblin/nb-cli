// #region Imports
use std::io::Write as _;
use std::sync::Arc;
use tokio::sync::Notify;
// #endregion

// #region Variables
const FRAMES: &[&str] = &["\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280F}"];
pub const CYAN: &str = "\x1b[36m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const RESET: &str = "\x1b[0m";
pub const ERASE: &str = "\x1b[2K\r";
const CHECK: &str = "\u{2714}";
const CROSS: &str = "\u{2718}";
// #endregion

// #region Spinner

/// Background spinner that animates at ~80ms on stderr.
/// Update the message with `set_message()`, stop with `finish()`.
pub struct Spinner {
    msg: Arc<std::sync::Mutex<String>>,
    stop: Arc<Notify>,
    handle: tokio::task::JoinHandle<()>,
}

impl Spinner {
    pub fn start(initial_msg: &str) -> Self {
        let msg = Arc::new(std::sync::Mutex::new(initial_msg.to_string()));
        let stop = Arc::new(Notify::new());

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

        Self { msg, stop, handle }
    }

    pub fn set_message(&self, new_msg: &str) {
        *self.msg.lock().unwrap() = new_msg.to_string();
    }

    pub async fn finish(self, msg: &str, ok: bool) {
        self.stop.notify_one();
        let _ = self.handle.await;
        let icon = if ok { format!("{GREEN}{CHECK}{RESET}") } else { format!("{RED}{CROSS}{RESET}") };
        eprintln!("{ERASE}  {icon} {DIM}{msg}{RESET}");
    }
}

// #endregion
