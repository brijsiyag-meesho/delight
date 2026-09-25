//! Thin AppKit shims GPUI doesn't expose: menu-bar-only activation policy,
//! a rounded vibrancy panel (quick-entry overlay look), top-anchored
//! resizing, and show/hide without closing the window.

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2 macros reference `cargo-clippy`
mod mac {
    use cocoa::base::{NO, YES, id, nil};
    use cocoa::foundation::{NSPoint, NSRect, NSSize};
    use gpui::Window;
    use objc::runtime::{self, BOOL, Class, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    fn ns_view(window: &Window) -> Option<id> {
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr() as id),
            _ => None,
        }
    }

    fn ns_window(window: &Window) -> Option<id> {
        let view = ns_view(window)?;
        let w: id = unsafe { msg_send![view, window] };
        (w != nil).then_some(w)
    }

    /// No Dock icon, no app menu — lives in the menu bar only.
    pub fn set_accessory_app() {
        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            // NSApplicationActivationPolicyAccessory = 1
            let _: () = msg_send![app, setActivationPolicy: 1i64];
        }
    }

    /// The launcher's vibrancy backdrop (one launcher window per process).
    static BACKDROP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    /// The Spotlight / quick-entry panel look: a native vibrancy backdrop
    /// (NSVisualEffectView) behind GPUI's view, shaped by a rounded mask, with
    /// a system drop shadow that follows it. The window itself is transparent
    /// — a GPUI "blurred" background is clipped by macOS to its own corner
    /// radius, not ours.
    pub fn style_floating_panel(window: &Window, radius: f64) {
        let Some(win) = ns_window(window) else { return };
        unsafe {
            let content: id = msg_send![win, contentView];
            let frame_view: id = msg_send![content, superview];
            if frame_view != nil && BACKDROP.get().is_none() {
                let bounds: NSRect = msg_send![frame_view, bounds];
                let effect: id = msg_send![class!(NSVisualEffectView), alloc];
                let effect: id = msg_send![effect, initWithFrame: bounds];
                // NSVisualEffectMaterialPopover, blending behind the window, always active.
                let _: () = msg_send![effect, setMaterial: 6i64];
                let _: () = msg_send![effect, setBlendingMode: 0i64];
                let _: () = msg_send![effect, setState: 1i64];
                // NSViewWidthSizable | NSViewHeightSizable
                let _: () = msg_send![effect, setAutoresizingMask: 18u64];
                // NSWindowBelow = -1
                let _: () = msg_send![frame_view, addSubview: effect positioned: -1i64 relativeTo: content];
                let _ = BACKDROP.set(effect as usize);
            }
            // We decide when to hide (Esc / blur / shortcut), not AppKit.
            let _: () = msg_send![win, setHidesOnDeactivate: NO];
            let _: () = msg_send![win, setOpaque: NO];
            let _: () = msg_send![win, setHasShadow: YES];
            let _: () = msg_send![win, setMovableByWindowBackground: YES];
        }
        set_corner_radius(window, radius);
    }

    /// Stretchable rounded-rect mask (9-slice via cap insets) for the backdrop.
    unsafe fn rounded_mask(radius: f64) -> id {
        let side = radius * 2. + 1.;
        let size = NSSize::new(side, side);
        unsafe {
            let image: id = msg_send![class!(NSImage), alloc];
            let image: id = msg_send![image, initWithSize: size];
            let _: () = msg_send![image, lockFocus];
            let black: id = msg_send![class!(NSColor), blackColor];
            let _: () = msg_send![black, set];
            let rect = NSRect::new(NSPoint::new(0., 0.), size);
            let path: id = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: rect xRadius: radius yRadius: radius];
            let _: () = msg_send![path, fill];
            let _: () = msg_send![image, unlockFocus];
            #[repr(C)]
            struct EdgeInsets(f64, f64, f64, f64);
            let _: () = msg_send![image, setCapInsets: EdgeInsets(radius, radius, radius, radius)];
            // NSImageResizingModeStretch = 1
            let _: () = msg_send![image, setResizingMode: 1i64];
            image
        }
    }

    /// Shape backdrop and content: a pill for the bar, a rounded panel once
    /// expanded (continuous "squircle" corners on the content layers).
    pub fn set_corner_radius(window: &Window, radius: f64) {
        let (Some(view), Some(win)) = (ns_view(window), ns_window(window)) else { return };
        unsafe {
            if let Some(&effect) = BACKDROP.get() {
                let _: () = msg_send![effect as id, setMaskImage: rounded_mask(radius)];
            }
            let content: id = msg_send![win, contentView];
            let curve: id = msg_send![class!(NSString), stringWithUTF8String: c"continuous".as_ptr()];
            for v in [content, view] {
                let _: () = msg_send![v, setWantsLayer: YES];
                let layer: id = msg_send![v, layer];
                if layer == nil {
                    continue;
                }
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: YES];
                let responds: bool = msg_send![layer, respondsToSelector: sel!(setCornerCurve:)];
                if responds {
                    let _: () = msg_send![layer, setCornerCurve: curve];
                }
            }
            let _: () = msg_send![win, invalidateShadow];
        }
    }

    /// Resize keeping the top edge fixed and the window horizontally centred
    /// (AppKit anchors bottom-left), so the panel grows down and outward from
    /// the input bar.
    pub fn resize_keep_top(window: &Window, width: f64, height: f64, animate: bool) {
        let Some(win) = ns_window(window) else { return };
        unsafe {
            let frame: NSRect = msg_send![win, frame];
            let content: NSRect = msg_send![win, contentRectForFrameRect: frame];
            let chrome = frame.size.height - content.size.height;
            let new_h = height + chrome;
            if (new_h - frame.size.height).abs() < 0.5 && (width - frame.size.width).abs() < 0.5 {
                return;
            }
            let top = frame.origin.y + frame.size.height;
            let x = frame.origin.x + (frame.size.width - width) / 2.;
            let new_frame = NSRect::new(NSPoint::new(x, top - new_h), NSSize::new(width, new_h));
            if animate {
                // The animator proxy animates asynchronously (no nested run loop).
                let animator: id = msg_send![win, animator];
                let _: () = msg_send![animator, setFrame: new_frame display: YES];
            } else {
                let _: () = msg_send![win, setFrame: new_frame display: YES animate: NO];
            }
            let _: () = msg_send![win, invalidateShadow];
        }
    }

    /// Opaque handle to the NSWindow, usable outside a GPUI update.
    pub fn native_window(window: &Window) -> Option<usize> {
        ns_window(window).map(|w| w as usize)
    }

    /// Bring the launcher forward as the key window *without* activating the
    /// app, like Spotlight: the app you were in stays active. Call outside any
    /// GPUI update — AppKit calls back into GPUI synchronously.
    pub fn present(native_window: usize) {
        let win = native_window as id;
        unsafe {
            let _: () = msg_send![win, orderFrontRegardless];
            let _: () = msg_send![win, makeKeyWindow];
        }
    }

    // ---------------------------------------------------------------------
    // GPUI 0.2.2 deadlock workaround
    //
    // When a window becomes key while the app is inactive — the launcher panel
    // (WindowKind::PopUp) shown by the hotkey, or Settings opened from the
    // menu bar — AppKit reports `isKeyWindow == NO` inside its own
    // `windowDidBecomeKey:`. GPUI treats that as spurious and calls
    // `resignKeyWindow` while still holding its window-state lock; the resign
    // notification re-enters the same handler and deadlocks the main thread
    // (the app freezes).
    //
    // We wrap `windowDidBecomeKey:` on both of GPUI's window classes: forward
    // to GPUI only once the window really is key (now, or on the next run-loop
    // turn), and otherwise drop the notification.
    // ---------------------------------------------------------------------

    type DidBecomeKey = extern "C" fn(&Object, Sel, id);
    static ORIGINAL_DID_BECOME_KEY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    fn call_original(this: &Object, notification: id) {
        if let Some(&imp) = ORIGINAL_DID_BECOME_KEY.get() {
            let original: DidBecomeKey = unsafe { std::mem::transmute(imp) };
            original(this, sel!(windowDidBecomeKey:), notification);
        }
    }

    extern "C" fn did_become_key(this: &Object, _: Sel, notification: id) {
        let is_key: BOOL = unsafe { msg_send![this, isKeyWindow] };
        if is_key == YES {
            call_original(this, notification);
        } else {
            unsafe {
                let _: () = msg_send![this, performSelector: sel!(delightDidBecomeKeyLater:) withObject: notification afterDelay: 0.0f64];
            }
        }
    }

    extern "C" fn did_become_key_later(this: &Object, _: Sel, notification: id) {
        let is_key: BOOL = unsafe { msg_send![this, isKeyWindow] };
        if is_key == YES {
            call_original(this, notification);
        }
    }

    /// Install once, after GPUI has registered its window classes (i.e. after
    /// the first window is opened).
    pub fn patch_gpui_focus() {
        if ORIGINAL_DID_BECOME_KEY.get().is_some() {
            return;
        }
        for name in ["GPUIPanel", "GPUIWindow"] {
            if let Some(class) = Class::get(name) {
                patch_class(class);
            }
        }
    }

    fn patch_class(class: &Class) {
        unsafe {
            let method = runtime::class_getInstanceMethod(class, sel!(windowDidBecomeKey:));
            if method.is_null() {
                return;
            }
            // Both classes share GPUI's one handler, so one original serves both.
            let original = runtime::method_getImplementation(method) as usize;
            if *ORIGINAL_DID_BECOME_KEY.get_or_init(|| original) != original {
                return;
            }
            let replacement: DidBecomeKey = did_become_key;
            runtime::method_setImplementation(method as *mut _, std::mem::transmute::<DidBecomeKey, runtime::Imp>(replacement));
            let later: DidBecomeKey = did_become_key_later;
            runtime::class_addMethod(
                class as *const _ as *mut _,
                sel!(delightDidBecomeKeyLater:),
                std::mem::transmute::<DidBecomeKey, runtime::Imp>(later),
                c"v@:@".as_ptr(),
            );
        }
    }

    /// Hide without destroying the window, so all state survives.
    pub fn hide_window(window: &Window) {
        if let Some(win) = ns_window(window) {
            unsafe {
                let _: () = msg_send![win, orderOut: nil];
            }
        }
    }

    pub fn is_window_visible(window: &Window) -> bool {
        ns_window(window).is_some_and(|win| unsafe {
            let v: bool = msg_send![win, isVisible];
            v
        })
    }
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(not(target_os = "macos"))]
mod other {
    use gpui::Window;
    pub fn set_accessory_app() {}
    pub fn style_floating_panel(_: &Window, _: f64) {}
    pub fn set_corner_radius(_: &Window, _: f64) {}
    pub fn resize_keep_top(window: &mut Window, width: f64, height: f64, _: bool) {
        window.resize(gpui::size(gpui::px(width as f32), gpui::px(height as f32)));
    }
    pub fn hide_window(_: &Window) {}
    pub fn native_window(_: &Window) -> Option<usize> {
        None
    }
    pub fn present(_: usize) {}
    pub fn patch_gpui_focus() {}
    pub fn is_window_visible(_: &Window) -> bool {
        true
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::*;
