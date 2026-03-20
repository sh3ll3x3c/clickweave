#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

pub use clickweave_core::MouseButton;

/// Raw capture event produced by the platform event tap.
///
/// Lightweight event sent from the OS event tap thread to the async processing
/// loop. The processing loop enriches these (via MCP accessibility/screenshot
/// calls) before wrapping them into `WalkthroughEvent` values.
#[derive(Debug, Clone)]
pub struct CaptureEvent {
    pub kind: CaptureEventKind,
    /// PID of the process that the event targets.
    pub target_pid: i32,
    /// Milliseconds since Unix epoch.
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub enum CaptureEventKind {
    MouseClick {
        x: f64,
        y: f64,
        button: MouseButton,
        click_count: u32,
        modifiers: Vec<String>,
    },
    KeyDown {
        /// Human-readable key name (e.g. "Return", "Tab", "a").
        key_name: String,
        /// Unicode characters produced by the key event, if any.
        characters: Option<String>,
        modifiers: Vec<String>,
    },
    ScrollWheel {
        delta_y: f64,
        x: f64,
        y: f64,
    },
}

/// Commands sent from the async processing loop to the event tap thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureCommand {
    Pause,
    Resume,
    Stop,
}
