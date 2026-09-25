//! Catches a panic while a plugin's view renders, lays out or paints.
//!
//! GPUI is mid-frame when that happens (its element stacks and the view's
//! entity are left behind), so the crash is fatal (`delight_core::guard`):
//! the plugin is turned off and Delight relaunches instead of aborting.

use std::panic::Location;

use delight_core::guard;
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    Window,
};

pub struct PanicBoundary {
    child: AnyElement,
    plugin_id: String,
    plugin_name: String,
}

impl PanicBoundary {
    pub fn new(child: AnyElement, plugin_id: impl Into<String>, plugin_name: impl Into<String>) -> Self {
        Self { child, plugin_id: plugin_id.into(), plugin_name: plugin_name.into() }
    }

    fn guard<R>(&mut self, f: impl FnOnce(&mut AnyElement) -> R) -> R {
        let child = &mut self.child;
        guard::catch(|| f(child)).unwrap_or_else(|message| guard::fatal(&self.plugin_id, &self.plugin_name, message))
    }
}

impl IntoElement for PanicBoundary {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Transparent: the child's layout is the boundary's (like `AnyElement`'s own
/// `Element` impl).
impl Element for PanicBoundary {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.guard(|child| child.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.guard(|child| {
            child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.guard(|child| child.paint(window, cx));
    }
}
