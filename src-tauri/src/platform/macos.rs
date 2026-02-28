use core_foundation::base::TCFType;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField, KeyCode,
};
use foreign_types::ForeignType;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use tokio::sync::mpsc;
use tracing::{error, info};

use super::{CaptureCommand, CaptureEvent, CaptureEventKind, MouseButton};

/// Handle for a running macOS event tap.
///
/// The tap runs on a dedicated std::thread with its own CFRunLoop.
/// Events are sent through a tokio mpsc channel to the async processing loop.
pub struct MacOSEventTap {
    thread: Option<std::thread::JoinHandle<()>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    /// Raw CFRunLoopRef for the tap thread, used to wake/stop the run loop
    /// from the control thread. `CFRunLoopStop` is thread-safe.
    run_loop: Arc<AtomicPtr<std::ffi::c_void>>,
}

impl MacOSEventTap {
    /// Start a passive (listen-only) event tap on a background thread.
    ///
    /// Returns the tap handle and a receiver for captured events.
    /// The tap captures: mouse clicks, key down events, scroll wheel events.
    pub fn start() -> Result<(Self, mpsc::UnboundedReceiver<CaptureEvent>), String> {
        let (tx, rx) = mpsc::unbounded_channel();
        let paused = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let run_loop = Arc::new(AtomicPtr::new(std::ptr::null_mut()));

        let paused_clone = paused.clone();
        let stopped_clone = stopped.clone();
        let run_loop_clone = run_loop.clone();

        let thread = std::thread::Builder::new()
            .name("walkthrough-event-tap".into())
            .spawn(move || {
                run_event_tap(tx, paused_clone, stopped_clone, run_loop_clone);
            })
            .map_err(|e| format!("Failed to spawn event tap thread: {e}"))?;

        Ok((
            Self {
                thread: Some(thread),
                paused,
                stopped,
                run_loop,
            },
            rx,
        ))
    }

    pub fn send_command(&self, cmd: CaptureCommand) {
        match cmd {
            CaptureCommand::Pause => self.paused.store(true, Ordering::SeqCst),
            CaptureCommand::Resume => self.paused.store(false, Ordering::SeqCst),
            CaptureCommand::Stop => {
                self.stopped.store(true, Ordering::SeqCst);
                self.wake_run_loop();
            }
        }
    }

    /// Stop the tap thread's CFRunLoop from any thread.
    /// `CFRunLoopStop` is thread-safe.
    fn wake_run_loop(&self) {
        let rl = self.run_loop.load(Ordering::SeqCst);
        if !rl.is_null() {
            unsafe {
                CFRunLoopStop(rl as *const _);
            }
        }
    }
}

impl Drop for MacOSEventTap {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.wake_run_loop();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_event_tap(
    tx: mpsc::UnboundedSender<CaptureEvent>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    run_loop_out: Arc<AtomicPtr<std::ffi::c_void>>,
) {
    let events_of_interest = vec![
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::KeyDown,
        CGEventType::ScrollWheel,
    ];

    let stopped_for_check = stopped.clone();
    let tap_result = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        events_of_interest,
        move |_proxy, event_type, event| {
            if stopped.load(Ordering::SeqCst) {
                CFRunLoop::get_current().stop();
                return None;
            }

            if paused.load(Ordering::SeqCst) {
                return None;
            }

            if let Some(kind) = translate_event(event_type, event) {
                let target_pid =
                    event.get_integer_value_field(EventField::EVENT_TARGET_UNIX_PROCESS_ID) as i32;
                let timestamp = clickweave_core::storage::now_millis();
                let capture_event = CaptureEvent {
                    kind,
                    target_pid,
                    timestamp,
                };
                if tx.send(capture_event).is_err() {
                    // Receiver dropped — stop the run loop.
                    CFRunLoop::get_current().stop();
                }
            }

            None // listen-only, don't modify events
        },
    );

    let tap = match tap_result {
        Ok(tap) => tap,
        Err(()) => {
            error!(
                "Failed to create CGEvent tap. \
                 Ensure Accessibility permissions are granted in \
                 System Settings > Privacy & Security > Accessibility."
            );
            return;
        }
    };

    let loop_source = match tap.mach_port.create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            error!("Failed to create CFRunLoop source for event tap");
            return;
        }
    };

    let current_loop = CFRunLoop::get_current();
    unsafe {
        current_loop.add_source(&loop_source, kCFRunLoopCommonModes);
    }

    // Publish the run loop ref so the control thread can call CFRunLoopStop.
    let raw_rl = current_loop.as_concrete_TypeRef() as *mut std::ffi::c_void;
    run_loop_out.store(raw_rl, Ordering::SeqCst);

    // Check if stop was already requested before we got here.
    if stopped_for_check.load(Ordering::SeqCst) {
        info!("macOS event tap: stop requested before run loop started");
        return;
    }

    tap.enable();

    info!("macOS event tap started");
    CFRunLoop::run_current();
    info!("macOS event tap stopped");
}

fn translate_event(event_type: CGEventType, event: &CGEvent) -> Option<CaptureEventKind> {
    match event_type {
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown => {
            let location = event.location();
            let click_count =
                event.get_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE) as u32;
            let button = match event_type {
                CGEventType::LeftMouseDown => MouseButton::Left,
                CGEventType::RightMouseDown => MouseButton::Right,
                _ => MouseButton::Center,
            };
            let modifiers = flags_to_modifiers(event.get_flags());

            Some(CaptureEventKind::MouseClick {
                x: location.x,
                y: location.y,
                button,
                click_count,
                modifiers,
            })
        }

        CGEventType::KeyDown => {
            // Skip auto-repeat events.
            let is_repeat =
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;
            if is_repeat {
                return None;
            }

            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
            let modifiers = flags_to_modifiers(event.get_flags());
            let key_name = keycode_to_name(keycode);
            let characters = get_unicode_string(event);

            Some(CaptureEventKind::KeyDown {
                key_name,
                characters,
                modifiers,
            })
        }

        CGEventType::ScrollWheel => {
            let delta_y = event
                .get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
            let location = event.location();

            // Ignore tiny accidental scrolls.
            if delta_y.abs() < 0.5 {
                return None;
            }

            Some(CaptureEventKind::ScrollWheel {
                delta_y,
                x: location.x,
                y: location.y,
            })
        }

        _ => None,
    }
}

fn flags_to_modifiers(flags: CGEventFlags) -> Vec<String> {
    let mut mods = Vec::new();
    if flags.contains(CGEventFlags::CGEventFlagCommand) {
        mods.push("command".to_string());
    }
    if flags.contains(CGEventFlags::CGEventFlagShift) {
        mods.push("shift".to_string());
    }
    if flags.contains(CGEventFlags::CGEventFlagAlternate) {
        mods.push("option".to_string());
    }
    if flags.contains(CGEventFlags::CGEventFlagControl) {
        mods.push("control".to_string());
    }
    mods
}

fn keycode_to_name(keycode: u16) -> String {
    match keycode {
        KeyCode::RETURN => "Return".to_string(),
        KeyCode::TAB => "Tab".to_string(),
        KeyCode::SPACE => "Space".to_string(),
        KeyCode::DELETE => "Delete".to_string(),
        KeyCode::ESCAPE => "Escape".to_string(),
        KeyCode::LEFT_ARROW => "Left".to_string(),
        KeyCode::RIGHT_ARROW => "Right".to_string(),
        KeyCode::DOWN_ARROW => "Down".to_string(),
        KeyCode::UP_ARROW => "Up".to_string(),
        KeyCode::HOME => "Home".to_string(),
        KeyCode::END => "End".to_string(),
        KeyCode::PAGE_UP => "PageUp".to_string(),
        KeyCode::PAGE_DOWN => "PageDown".to_string(),
        KeyCode::FORWARD_DELETE => "ForwardDelete".to_string(),
        KeyCode::F1 => "F1".to_string(),
        KeyCode::F2 => "F2".to_string(),
        KeyCode::F3 => "F3".to_string(),
        KeyCode::F4 => "F4".to_string(),
        KeyCode::F5 => "F5".to_string(),
        KeyCode::F6 => "F6".to_string(),
        KeyCode::F7 => "F7".to_string(),
        KeyCode::F8 => "F8".to_string(),
        KeyCode::F9 => "F9".to_string(),
        KeyCode::F10 => "F10".to_string(),
        KeyCode::F11 => "F11".to_string(),
        KeyCode::F12 => "F12".to_string(),
        _ => format!("0x{keycode:02X}"),
    }
}

/// Get the Unicode string associated with a key event.
///
/// Uses the raw `CGEventKeyboardGetUnicodeString` C API since the
/// `core-graphics` crate only exposes the setter, not the getter.
fn get_unicode_string(event: &CGEvent) -> Option<String> {
    let mut buf = [0u16; 8];
    let mut actual_len: u64 = 0;

    unsafe {
        CGEventKeyboardGetUnicodeString(
            event.as_ptr(),
            buf.len() as u64,
            &mut actual_len,
            buf.as_mut_ptr(),
        );
    }

    let len = actual_len as usize;
    if len == 0 {
        return None;
    }

    String::from_utf16(&buf[..len]).ok()
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventKeyboardGetUnicodeString(
        event: core_graphics::sys::CGEventRef,
        max_len: u64,
        actual_len: *mut u64,
        buf: *mut u16,
    );
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopStop(rl: *const std::ffi::c_void);
}
