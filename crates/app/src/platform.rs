//! Small AppKit calls GPUI doesn't expose.

/// No Dock icon, no app menu — Delight lives in the menu bar only.
#[cfg(target_os = "macos")]
pub fn set_accessory_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else { return };
    NSApplication::sharedApplication(main_thread).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

#[cfg(not(target_os = "macos"))]
pub fn set_accessory_app() {}
