//! AppKit calls GPUI doesn't expose: the menu-bar-only app, the launcher's
//! panel look (a rounded vibrancy backdrop), resizing that keeps the top edge
//! in place, showing and hiding without closing, and a GPUI focus fix.

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(not(target_os = "macos"))]
pub use other::*;

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::OnceLock;

    use gpui::Window;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, Message, msg_send, sel};
    use objc2_app_kit::{
        NSAnimatablePropertyContainer, NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions,
        NSBezierPath, NSColor, NSGlassEffectView, NSImage, NSImageResizingMode, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
    };
    use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    fn main_thread() -> Option<MainThreadMarker> {
        MainThreadMarker::new()
    }

    /// GPUI's view: it draws the window and takes the keyboard.
    fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else {
            return None;
        };
        // SAFETY: GPUI's AppKit handle points at the window's live NSView.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        Some(view.retain())
    }

    /// The NSWindow behind a GPUI window.
    fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
        gpui_view(window)?.window()
    }

    /// No Dock icon, no app menu — Delight lives in the menu bar only.
    pub fn set_accessory_app() {
        let Some(mtm) = main_thread() else { return };
        NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    /// Draws Delight's windows light (`Some(false)`), dark (`Some(true)`), or
    /// as macOS does (`None`) — so native parts (the blur, the glass, the
    /// title bar) match a theme chosen in Settings.
    pub fn set_app_appearance(dark: Option<bool>) {
        let Some(mtm) = main_thread() else { return };
        // SAFETY: AppKit's appearance name constants.
        let appearance = dark.and_then(|dark| {
            let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
            NSAppearance::appearanceNamed(name)
        });
        NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
    }

    /// The material behind the launcher's content.
    enum Backdrop {
        /// Liquid Glass (macOS 26+), as Spotlight uses: its own translucent
        /// fill, light rim and rounded shape.
        Glass(Retained<NSGlassEffectView>),
        /// Older macOS: a blur, shaped by a rounded mask.
        Blur(Retained<NSVisualEffectView>),
    }

    thread_local! {
        /// The launcher's backdrop (there's one launcher window).
        static BACKDROP: std::cell::RefCell<Option<Backdrop>> = const { std::cell::RefCell::new(None) };
    }

    fn new_backdrop(mtm: MainThreadMarker, frame: NSRect) -> (Backdrop, Retained<NSView>) {
        // NSGlassEffectView exists only on macOS 26+.
        if AnyClass::get(c"NSGlassEffectView").is_some() {
            let glass = NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(mtm), frame);
            let view = Retained::clone(&glass).into_super();
            return (Backdrop::Glass(glass), view);
        }
        let blur = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
        blur.setMaterial(NSVisualEffectMaterial::Popover);
        blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        blur.setState(NSVisualEffectState::Active);
        let view = Retained::clone(&blur).into_super();
        (Backdrop::Blur(blur), view)
    }

    /// Whether the launcher sits on Liquid Glass (it then draws a light rim).
    pub fn uses_liquid_glass() -> bool {
        BACKDROP.with_borrow(|backdrop| matches!(backdrop, Some(Backdrop::Glass(_))))
    }

    /// The Spotlight panel look: a native backdrop behind GPUI's view, with a
    /// system shadow that follows its shape. (GPUI's own blurred background
    /// keeps macOS's corner radius, not ours.)
    pub fn style_floating_panel(window: &Window, radius: f64) {
        let (Some(mtm), Some(win)) = (main_thread(), ns_window(window)) else { return };
        // Borderless: GPUI makes a titled window, and macOS draws a titled
        // window's own rounded frame and edge line around our shape. (GPUI's
        // windows can become key without a title bar.)
        win.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
        // Changing the style rebuilds the window's frame and hands the
        // keyboard to the window itself: give it back to GPUI's view, or
        // every key press just beeps.
        if let Some(view) = gpui_view(window) {
            win.makeFirstResponder(Some(&view));
        }
        let content = win.contentView();
        let has_backdrop = BACKDROP.with_borrow(Option::is_some);
        if let (Some(content), false) = (&content, has_backdrop)
            && let Some(frame_view) = unsafe { content.superview() }
        {
            let (backdrop, view) = new_backdrop(mtm, frame_view.bounds());
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            frame_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(content));
            BACKDROP.set(Some(backdrop));
        }
        // Delight decides when to hide (Esc, losing focus, the shortcut).
        win.setHidesOnDeactivate(false);
        win.setOpaque(false);
        win.setHasShadow(true);
        // Dragged only by its handles (see `drag_window`), so text can be selected.
        win.setMovableByWindowBackground(false);
        set_corner_radius(window, radius);
    }

    /// A stretchable rounded-rect mask image (its corners stay fixed).
    fn rounded_mask(radius: f64) -> Retained<NSImage> {
        let side = radius * 2. + 1.;
        let size = NSSize::new(side, side);
        let image = NSImage::initWithSize(NSImage::alloc(), size);
        #[allow(deprecated)] // lockFocus: the simplest way to draw into an image
        {
            image.lockFocus();
            NSColor::blackColor().set();
            let rect = NSRect::new(NSPoint::new(0., 0.), size);
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
            image.unlockFocus();
        }
        image.setCapInsets(NSEdgeInsets { top: radius, left: radius, bottom: radius, right: radius });
        image.setResizingMode(NSImageResizingMode::Stretch);
        image
    }

    /// Rounds the backdrop and GPUI's content (continuous "squircle" corners).
    pub fn set_corner_radius(window: &Window, radius: f64) {
        let Some(win) = ns_window(window) else { return };
        BACKDROP.with_borrow(|backdrop| match backdrop {
            Some(Backdrop::Glass(glass)) => glass.setCornerRadius(radius),
            Some(Backdrop::Blur(blur)) => blur.setMaskImage(Some(&rounded_mask(radius))),
            None => {}
        });
        // Clip GPUI's content — and the glass, whose effect otherwise reaches
        // into the window's square corners — to the same rounded shape.
        let glass = BACKDROP.with_borrow(|backdrop| match backdrop {
            Some(Backdrop::Glass(glass)) => Some(Retained::clone(glass).into_super()),
            _ => None,
        });
        let continuous = NSString::from_str("continuous");
        for view in win.contentView().into_iter().chain(glass) {
            view.setWantsLayer(true);
            let Some(layer) = view.layer() else { continue };
            // SAFETY: plain CALayer property setters.
            unsafe {
                let _: () = msg_send![&*layer, setCornerRadius: radius];
                let _: () = msg_send![&*layer, setMasksToBounds: true];
                let _: () = msg_send![&*layer, setCornerCurve: &*continuous];
            }
        }
        win.invalidateShadow();
    }

    /// Resizes keeping the top edge fixed and the window centred (AppKit
    /// anchors the bottom-left), so the panel grows down from the input bar.
    pub fn resize_keep_top(window: &Window, width: f64, height: f64, animate: bool) {
        let Some(win) = ns_window(window) else { return };
        let frame = win.frame();
        let chrome = frame.size.height - win.contentRectForFrameRect(frame).size.height;
        let new_height = height + chrome;
        if (new_height - frame.size.height).abs() < 0.5 && (width - frame.size.width).abs() < 0.5 {
            return;
        }
        let top = frame.origin.y + frame.size.height;
        let x = frame.origin.x + (frame.size.width - width) / 2.;
        let new_frame = NSRect::new(NSPoint::new(x, top - new_height), NSSize::new(width, new_height));
        if animate {
            // The animator proxy animates asynchronously (no nested run loop).
            win.animator().setFrame_display(new_frame, true);
        } else {
            win.setFrame_display(new_frame, true);
        }
        win.invalidateShadow();
    }

    /// The NSWindow as a plain handle, for use outside a GPUI update.
    pub fn native_window(window: &Window) -> Option<usize> {
        ns_window(window).map(|w| Retained::into_raw(w) as usize)
    }

    /// Brings the window forward as the key window *without* activating the
    /// app, like Spotlight: the app you were in stays active. Call it outside
    /// any GPUI update — AppKit calls back into GPUI synchronously.
    pub fn present(native_window: usize) {
        // SAFETY: from `native_window`, which kept a +1 reference; taken back here.
        let Some(win) = (unsafe { Retained::from_raw(native_window as *mut NSWindow) }) else { return };
        win.orderFrontRegardless();
        win.makeKeyWindow();
    }

    /// Moves the window with the mouse. Call it from a mouse-down handler
    /// (it uses the mouse-down event AppKit is delivering).
    pub fn drag_window(window: &Window) {
        let (Some(mtm), Some(win)) = (main_thread(), ns_window(window)) else { return };
        if let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() {
            win.performWindowDragWithEvent(&event);
        }
    }

    /// Hides without closing, so all state survives.
    pub fn hide_window(window: &Window) {
        if let Some(win) = ns_window(window) {
            win.orderOut(None);
        }
    }

    pub fn is_window_visible(window: &Window) -> bool {
        ns_window(window).is_some_and(|win| win.isVisible())
    }

    // -------------------------------------------------------------------------
    // GPUI 0.2.2 freeze workaround
    //
    // When a window becomes key while the app is inactive (the launcher shown
    // by the hotkey), AppKit reports `isKeyWindow == NO` inside its own
    // `windowDidBecomeKey:`. GPUI treats that as spurious and calls
    // `resignKeyWindow` while holding its window-state lock; the resign
    // notification re-enters the same handler and deadlocks the main thread.
    //
    // So `windowDidBecomeKey:` on GPUI's window classes is wrapped: forwarded
    // to GPUI only once the window really is key (now, or on the next
    // run-loop turn), dropped otherwise.
    // -------------------------------------------------------------------------

    type DidBecomeKey = unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);

    static GPUI_DID_BECOME_KEY: OnceLock<Imp> = OnceLock::new();

    unsafe fn forward_to_gpui(this: &AnyObject, notification: *mut AnyObject) {
        if let Some(&imp) = GPUI_DID_BECOME_KEY.get() {
            // SAFETY: GPUI's implementation of this very selector.
            unsafe {
                let gpui: DidBecomeKey = std::mem::transmute(imp);
                gpui(this, sel!(windowDidBecomeKey:), notification);
            }
        }
    }

    fn is_key(this: &AnyObject) -> bool {
        unsafe { msg_send![this, isKeyWindow] }
    }

    unsafe extern "C-unwind" fn did_become_key(this: &AnyObject, _: Sel, notification: *mut AnyObject) {
        if is_key(this) {
            unsafe { forward_to_gpui(this, notification) };
        } else {
            unsafe {
                let _: () = msg_send![this, performSelector: sel!(delightDidBecomeKeyLater:), withObject: notification, afterDelay: 0.0f64];
            }
        }
    }

    unsafe extern "C-unwind" fn did_become_key_later(this: &AnyObject, _: Sel, notification: *mut AnyObject) {
        if is_key(this) {
            unsafe { forward_to_gpui(this, notification) };
        }
    }

    /// Installs the workaround. Call it once GPUI has registered its window
    /// classes, i.e. after opening the first window.
    pub fn patch_gpui_focus() {
        if GPUI_DID_BECOME_KEY.get().is_some() {
            return;
        }
        for name in [c"GPUIPanel", c"GPUIWindow"] {
            let Some(class) = AnyClass::get(name) else { continue };
            let Some(method) = class.instance_method(sel!(windowDidBecomeKey:)) else { continue };
            // Both classes share one GPUI handler: patch each class once.
            let original = method.implementation();
            if !std::ptr::fn_addr_eq(*GPUI_DID_BECOME_KEY.get_or_init(|| original), original) {
                continue;
            }
            // SAFETY: the replacements have the selector's signature (v@:@).
            unsafe {
                method.set_implementation(std::mem::transmute::<DidBecomeKey, Imp>(did_become_key));
                objc2::ffi::class_addMethod(
                    class as *const AnyClass as *mut AnyClass,
                    sel!(delightDidBecomeKeyLater:),
                    std::mem::transmute::<DidBecomeKey, Imp>(did_become_key_later),
                    c"v@:@".as_ptr(),
                );
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use gpui::Window;

    pub fn set_accessory_app() {}
    pub fn style_floating_panel(_: &Window, _: f64) {}
    pub fn set_corner_radius(_: &Window, _: f64) {}
    pub fn resize_keep_top(window: &Window, width: f64, height: f64, _: bool) {
        let _ = (window, width, height);
    }
    pub fn native_window(_: &Window) -> Option<usize> {
        None
    }
    pub fn present(_: usize) {}
    pub fn hide_window(_: &Window) {}
    pub fn drag_window(_: &Window) {}
    pub fn is_window_visible(_: &Window) -> bool {
        true
    }
    pub fn patch_gpui_focus() {}
    pub fn set_app_appearance(_: Option<bool>) {}
    pub fn uses_liquid_glass() -> bool {
        false
    }
}
