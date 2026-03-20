#[allow(unused_imports)]
use std::cell::RefCell;
#[allow(unused_imports)]
use std::collections::HashSet;
#[allow(unused_imports)]
use std::sync::Arc;
#[allow(unused_imports)]
use std::sync::atomic::{AtomicBool, Ordering};
#[allow(unused_imports)]
use tokio::sync::mpsc;
#[allow(unused_imports)]
use tracing::{error, info};

#[allow(unused_imports)]
use super::{CaptureCommand, CaptureEvent, CaptureEventKind, MouseButton};

/// Half-size of the cursor region capture in screen points.
/// 32pt → 64pt total region around the cursor.
#[allow(dead_code)]
pub const CURSOR_REGION_HALF_PT: f64 = 32.0;

/// A small screen region captured around the cursor position.
///
/// Stores raw RGBA pixels. The captured region IS the click crop template —
/// no secondary crop step is needed.
#[allow(dead_code)]
#[derive(Clone)]
pub struct CursorRegionCapture {
    /// Raw RGBA pixel data (4 bytes per pixel, row-major, top-down).
    pub rgba_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Virtual key → name mapping
// ---------------------------------------------------------------------------

/// Map a Windows virtual key code to the key name accepted by the MCP
/// `press_key` tool.
///
/// Names mirror the macOS `keycode_to_name()` mapping so that recorded
/// walkthrough events are platform-agnostic at the consumer level.
#[allow(dead_code)]
pub fn vk_to_name(vk: u16) -> String {
    match vk {
        // Special keys.
        0x0D => "return".to_string(),   // VK_RETURN
        0x09 => "tab".to_string(),      // VK_TAB
        0x20 => "space".to_string(),    // VK_SPACE
        0x08 => "delete".to_string(),   // VK_BACK (backspace → "delete" matching macOS)
        0x1B => "escape".to_string(),   // VK_ESCAPE
        0x25 => "left".to_string(),     // VK_LEFT
        0x26 => "up".to_string(),       // VK_UP
        0x27 => "right".to_string(),    // VK_RIGHT
        0x28 => "down".to_string(),     // VK_DOWN
        0x24 => "home".to_string(),     // VK_HOME
        0x23 => "end".to_string(),      // VK_END
        0x21 => "pageup".to_string(),   // VK_PRIOR
        0x22 => "pagedown".to_string(), // VK_NEXT
        0x2E => "forwarddelete".to_string(), // VK_DELETE (forward delete)

        // Function keys.
        0x70 => "f1".to_string(),
        0x71 => "f2".to_string(),
        0x72 => "f3".to_string(),
        0x73 => "f4".to_string(),
        0x74 => "f5".to_string(),
        0x75 => "f6".to_string(),
        0x76 => "f7".to_string(),
        0x77 => "f8".to_string(),
        0x78 => "f9".to_string(),
        0x79 => "f10".to_string(),
        0x7A => "f11".to_string(),
        0x7B => "f12".to_string(),

        // Letter keys (VK_A–VK_Z are 0x41–0x5A).
        0x41..=0x5A => {
            let ch = (b'a' + (vk as u8 - 0x41)) as char;
            ch.to_string()
        }

        // Digit keys (VK_0–VK_9 are 0x30–0x39).
        0x30..=0x39 => {
            let ch = (b'0' + (vk as u8 - 0x30)) as char;
            ch.to_string()
        }

        // Numpad digit keys (VK_NUMPAD0–VK_NUMPAD9 are 0x60–0x69).
        0x60..=0x69 => {
            let digit = vk - 0x60;
            format!("Numpad{digit}")
        }
        0x6A => "NumpadMultiply".to_string(), // VK_MULTIPLY
        0x6B => "NumpadPlus".to_string(),     // VK_ADD
        0x6D => "NumpadMinus".to_string(),    // VK_SUBTRACT
        0x6E => "NumpadDecimal".to_string(),  // VK_DECIMAL
        0x6F => "NumpadDivide".to_string(),   // VK_DIVIDE

        // OEM keys (US layout).
        0xBA => ";".to_string(),  // VK_OEM_1
        0xBB => "=".to_string(),  // VK_OEM_PLUS
        0xBC => ",".to_string(),  // VK_OEM_COMMA
        0xBD => "-".to_string(),  // VK_OEM_MINUS
        0xBE => ".".to_string(),  // VK_OEM_PERIOD
        0xBF => "/".to_string(),  // VK_OEM_2
        0xC0 => "`".to_string(),  // VK_OEM_3
        0xDB => "[".to_string(),  // VK_OEM_4
        0xDC => "\\".to_string(), // VK_OEM_5
        0xDD => "]".to_string(),  // VK_OEM_6
        0xDE => "'".to_string(),  // VK_OEM_7

        // Unknown key: emit hex code so nothing is silently dropped.
        _ => format!("0x{vk:02X}"),
    }
}

// ---------------------------------------------------------------------------
// Multi-click tracker
// ---------------------------------------------------------------------------

/// Tracks consecutive clicks to compute click count (single, double, triple…).
///
/// Two clicks are considered consecutive when they target the same mouse button,
/// occur within 500 ms of each other, and land within 4 px of each other.
#[allow(dead_code)]
pub struct ClickTracker {
    last_button: u32,
    last_x: i32,
    last_y: i32,
    last_time: u64,
    count: u32,
}

impl ClickTracker {
    /// Create a new tracker with no previous click recorded.
    pub fn new() -> Self {
        Self {
            last_button: u32::MAX,
            last_x: 0,
            last_y: 0,
            last_time: 0,
            count: 0,
        }
    }

    /// Register a click and return the current consecutive click count.
    ///
    /// `timestamp_ms` is milliseconds since some monotonic or epoch origin;
    /// only differences between timestamps matter.
    pub fn register_click(&mut self, button: u32, x: i32, y: i32, timestamp_ms: u64) -> u32 {
        const MAX_INTERVAL_MS: u64 = 500;
        const MAX_DISTANCE_PX: i32 = 4;

        let same_button = button == self.last_button;
        let within_time = timestamp_ms.saturating_sub(self.last_time) <= MAX_INTERVAL_MS;
        let dx = (x - self.last_x).abs();
        let dy = (y - self.last_y).abs();
        let within_distance = dx <= MAX_DISTANCE_PX && dy <= MAX_DISTANCE_PX;

        if same_button && within_time && within_distance {
            self.count += 1;
        } else {
            self.count = 1;
        }

        self.last_button = button;
        self.last_x = x;
        self.last_y = y;
        self.last_time = timestamp_ms;

        self.count
    }
}

// ---------------------------------------------------------------------------
// Pixel format conversion
// ---------------------------------------------------------------------------

/// Convert BGRA bottom-up pixels (GDI `GetDIBits` format) to RGBA top-down
/// pixels suitable for use with image encoders and the MCP screenshot path.
///
/// `bgra` must contain exactly `width * height * 4` bytes.
#[allow(dead_code)]
pub fn bgra_bottom_up_to_rgba(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width * 4) as usize;
    let mut rgba = Vec::with_capacity(bgra.len());

    // GDI bottom-up: row 0 in the buffer is the bottom row of the image.
    // Iterate rows in reverse to flip to top-down.
    for row in (0..height as usize).rev() {
        let start = row * row_bytes;
        let end = start + row_bytes;
        let row_slice = &bgra[start..end];
        for pixel in row_slice.chunks_exact(4) {
            rgba.push(pixel[2]); // R (from B at index 2)
            rgba.push(pixel[1]); // G
            rgba.push(pixel[0]); // B (from R at index 0)
            rgba.push(pixel[3]); // A
        }
    }

    rgba
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- vk_to_name ----------------------------------------------------------

    #[test]
    fn vk_to_name_return_key() {
        assert_eq!(vk_to_name(0x0D), "return");
    }

    #[test]
    fn vk_to_name_letter_keys() {
        assert_eq!(vk_to_name(0x41), "a");
        assert_eq!(vk_to_name(0x5A), "z");
        assert_eq!(vk_to_name(0x4D), "m");
    }

    #[test]
    fn vk_to_name_number_keys() {
        assert_eq!(vk_to_name(0x30), "0");
        assert_eq!(vk_to_name(0x39), "9");
        assert_eq!(vk_to_name(0x35), "5");
    }

    #[test]
    fn vk_to_name_arrow_keys() {
        assert_eq!(vk_to_name(0x25), "left");
        assert_eq!(vk_to_name(0x26), "up");
        assert_eq!(vk_to_name(0x27), "right");
        assert_eq!(vk_to_name(0x28), "down");
    }

    #[test]
    fn vk_to_name_function_keys() {
        assert_eq!(vk_to_name(0x70), "f1");
        assert_eq!(vk_to_name(0x7B), "f12");
        assert_eq!(vk_to_name(0x75), "f6");
    }

    #[test]
    fn vk_to_name_special_keys() {
        assert_eq!(vk_to_name(0x09), "tab");
        assert_eq!(vk_to_name(0x20), "space");
        assert_eq!(vk_to_name(0x08), "delete");
        assert_eq!(vk_to_name(0x1B), "escape");
        assert_eq!(vk_to_name(0x24), "home");
        assert_eq!(vk_to_name(0x23), "end");
        assert_eq!(vk_to_name(0x21), "pageup");
        assert_eq!(vk_to_name(0x22), "pagedown");
        assert_eq!(vk_to_name(0x2E), "forwarddelete");
    }

    #[test]
    fn vk_to_name_unknown_emits_hex() {
        assert_eq!(vk_to_name(0xFF), "0xFF");
        assert_eq!(vk_to_name(0x01), "0x01");
    }

    // --- ClickTracker --------------------------------------------------------

    #[test]
    fn click_tracker_first_click_returns_one() {
        let mut tracker = ClickTracker::new();
        let count = tracker.register_click(0, 100, 200, 1000);
        assert_eq!(count, 1);
    }

    #[test]
    fn click_tracker_double_click_same_position() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        let count = tracker.register_click(0, 100, 200, 1200);
        assert_eq!(count, 2);
    }

    #[test]
    fn click_tracker_triple_click_same_position() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        tracker.register_click(0, 100, 200, 1200);
        let count = tracker.register_click(0, 100, 200, 1400);
        assert_eq!(count, 3);
    }

    #[test]
    fn click_tracker_resets_after_timeout() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        // 501 ms later — exceeds the 500 ms threshold.
        let count = tracker.register_click(0, 100, 200, 1501);
        assert_eq!(count, 1);
    }

    #[test]
    fn click_tracker_resets_after_large_move() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        // Moved more than 4 px.
        let count = tracker.register_click(0, 110, 200, 1200);
        assert_eq!(count, 1);
    }

    #[test]
    fn click_tracker_resets_on_different_button() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        // Different button (right click).
        let count = tracker.register_click(1, 100, 200, 1200);
        assert_eq!(count, 1);
    }

    #[test]
    fn click_tracker_allows_small_move_within_threshold() {
        let mut tracker = ClickTracker::new();
        tracker.register_click(0, 100, 200, 1000);
        // Moved exactly 4 px — still within threshold.
        let count = tracker.register_click(0, 104, 204, 1200);
        assert_eq!(count, 2);
    }

    // --- bgra_bottom_up_to_rgba ----------------------------------------------

    #[test]
    fn bgra_bottom_up_to_rgba_2x2_image() {
        // 2×2 image, bottom-up BGRA:
        //   Row 0 (bottom of image): pixel (0,1) = BGRA(10,20,30,255), pixel (1,1) = BGRA(40,50,60,255)
        //   Row 1 (top of image):    pixel (0,0) = BGRA(70,80,90,255), pixel (1,0) = BGRA(100,110,120,255)
        #[rustfmt::skip]
        let bgra: Vec<u8> = vec![
            // Row 0 in buffer = bottom row of image
            10, 20, 30, 255,   // pixel (col=0, imageRow=1): B=10,G=20,R=30,A=255
            40, 50, 60, 255,   // pixel (col=1, imageRow=1): B=40,G=50,R=60,A=255
            // Row 1 in buffer = top row of image
            70, 80, 90, 255,   // pixel (col=0, imageRow=0): B=70,G=80,R=90,A=255
            100, 110, 120, 255,// pixel (col=1, imageRow=0): B=100,G=110,R=120,A=255
        ];

        let rgba = bgra_bottom_up_to_rgba(&bgra, 2, 2);

        // Expected output is top-down RGBA:
        // Top row (from buffer row 1): R=90,G=80,B=70,A=255  then R=120,G=110,B=100,A=255
        // Bottom row (from buffer row 0): R=30,G=20,B=10,A=255  then R=60,G=50,B=40,A=255
        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            90, 80, 70, 255,    // top-left
            120, 110, 100, 255, // top-right
            30, 20, 10, 255,    // bottom-left
            60, 50, 40, 255,    // bottom-right
        ];

        assert_eq!(rgba, expected);
    }

    #[test]
    fn bgra_bottom_up_to_rgba_preserves_length() {
        let bgra = vec![0u8; 4 * 3 * 5]; // 3×5 image
        let rgba = bgra_bottom_up_to_rgba(&bgra, 3, 5);
        assert_eq!(rgba.len(), bgra.len());
    }
}
