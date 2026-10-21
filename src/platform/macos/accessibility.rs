//! Bounded asynchronous macOS Accessibility traversal for UI hints.

use std::collections::{HashMap, HashSet};
use std::ffi::{c_float, c_int, c_void};
use std::ptr;
use std::time::{Duration, Instant};

use core_foundation::ConcreteCFType;
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use objc2_app_kit::NSWorkspace;

use crate::api::command::UiScanRequest;
use crate::api::geometry::{Rect, SemanticRole, UiTarget};

use super::native::OwnedCf;
use super::window_move::WriteError;
mod attribute_batch;
pub(super) mod window_manager;
mod window_tabs;

const AX_OK: i32 = 0;
const AX_CANNOT_COMPLETE: i32 = -25204;
const AX_VALUE_CGPOINT: i32 = 1;
const AX_VALUE_CGSIZE: i32 = 2;
// Apple declares AXUIElementSetMessagingTimeout's value as a C `float`.
// Keeping the constant typed prevents an accidental ABI-widening to `double`.
const NODE_TIMEOUT_SECONDS: c_float = 0.05;

type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;

struct AxApplication(OwnedCf);

impl AxApplication {
    fn new(pid: libc::pid_t) -> Result<Self, String> {
        Self::with_timeout(pid, NODE_TIMEOUT_SECONDS)
    }
    fn with_timeout(pid: libc::pid_t, timeout: c_float) -> Result<Self, String> {
        // SAFETY: AX returns a +1 application object for the pid; the owned
        // wrapper takes that create-rule reference exactly once. The timeout
        // is finite and is installed before any synchronous attribute query.
        let application =
            unsafe { OwnedCf::from_create_rule(AXUIElementCreateApplication(pid).cast()) }
                .ok_or_else(|| "cannot create AX application element".to_string())?;
        // SAFETY: `application` is a live AXUIElement and the timeout is finite.
        let error = unsafe { AXUIElementSetMessagingTimeout(application.as_ptr(), timeout) };
        if error != AX_OK {
            return Err(format!(
                "cannot set the AX messaging timeout for pid {pid}: AXError {error}"
            ));
        }
        // Some providers activate native accessibility on the application role
        // query, before exposing window contents. Use the same bounded, read-only
        // AX entry for every application; do not enable screen-reader modes.
        drop(copy_attribute(
            application.as_ptr(),
            &CFString::new("AXRole"),
        ));
        Ok(Self(application))
    }

    fn as_ptr(&self) -> *const c_void {
        self.0.as_ptr()
    }
}

struct AxAttributes {
    role: CFString,
    enabled: CFString,
    hidden: CFString,
    children: CFString,
    visible_children: CFString,
    visible_rows: CFString,
    position: CFString,
    size: CFString,
    title: CFString,
    description: CFString,
    help: CFString,
    focused_window: CFString,
}

impl AxAttributes {
    fn new() -> Self {
        Self {
            role: CFString::new("AXRole"),
            enabled: CFString::new("AXEnabled"),
            hidden: CFString::new("AXHidden"),
            children: CFString::new("AXChildren"),
            visible_children: CFString::new("AXVisibleChildren"),
            visible_rows: CFString::new("AXVisibleRows"),
            position: CFString::new("AXPosition"),
            size: CFString::new("AXSize"),
            title: CFString::new("AXTitle"),
            description: CFString::new("AXDescription"),
            help: CFString::new("AXHelp"),
            focused_window: CFString::new("AXFocusedWindow"),
        }
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(
        element: AXUIElementRef,
        x: c_float,
        y: c_float,
        result: *mut AXUIElementRef,
    ) -> c_int;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> c_int;
    fn AXValueCreate(value_type: c_int, value: *const c_void) -> AXValueRef;
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> c_int;
    fn AXUIElementCopyMultipleAttributeValues(
        element: AXUIElementRef,
        attributes: CFArrayRef,
        options: u32,
        values: *mut CFArrayRef,
    ) -> c_int;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut *const c_void) -> c_int;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: c_float) -> c_int;
    fn AXUIElementGetTypeID() -> usize;
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetType(value: AXValueRef) -> c_int;
    fn AXValueGetValue(value: AXValueRef, value_type: c_int, output: *mut c_void) -> bool;
}

pub(super) fn movable_focused_window() -> Result<Option<MovableWindow>, String> {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
        return Ok(None);
    };
    if app.processIdentifier() as u32 == std::process::id() {
        return Ok(None);
    }
    let application = AxApplication::new(app.processIdentifier())?;
    let attributes = AxAttributes::new();
    let Some(window) = copy_attribute(application.as_ptr(), &attributes.focused_window) else {
        return Ok(None);
    };
    if !is_ax_element(window.as_ptr()) || !is_ordinary_ax_window(window.as_ptr()) {
        return Ok(None);
    }
    let minimized = CFString::new("AXMinimized");
    if copy_bool_attribute(window.as_ptr(), &minimized) == Some(true)
        || element_rect(window.as_ptr(), &attributes).is_none()
    {
        return Ok(None);
    }
    Ok(Some(MovableWindow {
        window,
        attributes,
        fullscreen: CFString::new("AXFullScreen"),
        minimized,
    }))
}

/// Resolve the window from the physical pointer, never the focused window.
pub(super) fn window_under_pointer(
    cursor: crate::api::geometry::Point,
) -> Result<Option<MovableWindow>, String> {
    crate::platform::common::accessibility_window::with_inventory_fallback(
        ax_window_under_pointer(cursor),
        || window_manager::pointer_window_from_inventory(cursor),
    )
}

fn ax_window_under_pointer(cursor: crate::api::Point) -> Result<Option<MovableWindow>, String> {
    // SAFETY: AX Create returns a +1 object, transferred once to OwnedCf.
    let system = unsafe { OwnedCf::from_create_rule(AXUIElementCreateSystemWide()) }
        .ok_or_else(|| "cannot create AX system element".to_string())?;
    // SAFETY: the AX reference is live and the timeout is finite.
    let timeout = unsafe { AXUIElementSetMessagingTimeout(system.as_ptr(), NODE_TIMEOUT_SECONDS) };
    if timeout != AX_OK {
        return Err(format!(
            "cannot set window lookup timeout: AXError {timeout}"
        ));
    }
    let mut hit = ptr::null();
    // SAFETY: hit is a writable output and the returned +1 object is adopted
    // even if AX reports a failure, so every returned reference is released.
    let (error, hit) = unsafe {
        let error = AXUIElementCopyElementAtPosition(
            system.as_ptr(),
            cursor.x as c_float,
            cursor.y as c_float,
            &mut hit,
        );
        (error, OwnedCf::from_create_rule(hit))
    };
    if error != AX_OK {
        return Err(format!("cannot find window under pointer: AXError {error}"));
    }
    let Some(hit) = hit else {
        return Ok(None);
    };
    let lookup = ContainingWindowLookup {
        cursor,
        deadline: Instant::now() + Duration::from_millis(250),
        attributes: AxAttributes::new(),
        window: CFString::new("AXWindow"),
        top_level: CFString::new("AXTopLevelUIElement"),
        parent: CFString::new("AXParent"),
        minimized: CFString::new("AXMinimized"),
    };
    let Some(window) = crate::platform::common::accessibility_window::resolve(&lookup, hit) else {
        return Ok(None);
    };
    Ok(Some(MovableWindow {
        window,
        attributes: lookup.attributes,
        fullscreen: CFString::new("AXFullScreen"),
        minimized: CFString::new("AXMinimized"),
    }))
}

struct ContainingWindowLookup {
    cursor: crate::api::Point,
    deadline: Instant,
    attributes: AxAttributes,
    window: CFString,
    top_level: CFString,
    parent: CFString,
    minimized: CFString,
}

impl crate::platform::common::accessibility_window::Lookup for ContainingWindowLookup {
    type Node = OwnedCf;
    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
    fn same(&self, left: &OwnedCf, right: &OwnedCf) -> bool {
        // SAFETY: both retained CF objects are live during this comparison.
        unsafe { core_foundation::base::CFEqual(left.as_ptr(), right.as_ptr()) != 0 }
    }
    fn usable(&self, node: &OwnedCf) -> bool {
        if !is_ax_element(node.as_ptr()) || self.expired() {
            return false;
        }
        // SAFETY: type-checked retained AX object; install a finite timeout
        // before querying even the first hit/ancestor's role.
        if unsafe { AXUIElementSetMessagingTimeout(node.as_ptr(), NODE_TIMEOUT_SECONDS) } != AX_OK {
            return false;
        }
        copy_string_attribute(node.as_ptr(), &self.attributes.role).as_deref() == Some("AXWindow")
            && !self.expired()
            && copy_bool_attribute(node.as_ptr(), &self.attributes.hidden) != Some(true)
            && !self.expired()
            && copy_bool_attribute(node.as_ptr(), &self.minimized) != Some(true)
            && !self.expired()
            && element_rect(node.as_ptr(), &self.attributes)
                .is_some_and(|bounds| bounds.contains(&self.cursor))
    }
    fn related(
        &self,
        node: &OwnedCf,
        link: crate::platform::common::accessibility_window::Link,
    ) -> Option<OwnedCf> {
        use crate::platform::common::accessibility_window::Link;
        if !is_ax_element(node.as_ptr()) || self.expired() {
            return None;
        }
        // SAFETY: the source is a live AX element; failure must not leave
        // relationship queries using the application's unbounded default.
        if unsafe { AXUIElementSetMessagingTimeout(node.as_ptr(), NODE_TIMEOUT_SECONDS) } != AX_OK {
            return None;
        }
        copy_attribute(
            node.as_ptr(),
            match link {
                Link::Window => &self.window,
                Link::TopLevel => &self.top_level,
                Link::Parent => &self.parent,
            },
        )
        .filter(|value| is_ax_element(value.as_ptr()))
    }
}

fn is_ordinary_ax_window(window: AXUIElementRef) -> bool {
    crate::platform::common::accessibility_window::ordinary_role(
        copy_string_attribute(window, &CFString::new("AXRole")).as_deref(),
        copy_string_attribute(window, &CFString::new("AXSubrole")).as_deref(),
    )
}

pub(super) struct MovableWindow {
    window: OwnedCf,
    attributes: AxAttributes,
    fullscreen: CFString,
    minimized: CFString,
}

impl MovableWindow {
    fn set_attribute(&self, attribute: &CFString, value: CFTypeRef) -> Result<(), WriteError> {
        // SAFETY: the window, attribute and caller-owned value are live for the
        // bounded call. All callers supply an AXValue or CFBoolean CF object.
        let error = unsafe {
            AXUIElementSetAttributeValue(
                self.window.as_ptr(),
                attribute.as_concrete_TypeRef(),
                value,
            )
        };
        if error == AX_OK {
            Ok(())
        } else if error == AX_CANNOT_COMPLETE {
            Err(WriteError::Unconfirmed(format!(
                "cannot confirm window {attribute}: AXError {error}"
            )))
        } else {
            Err(WriteError::Rejected(format!(
                "cannot set window {attribute}: AXError {error}"
            )))
        }
    }
}

impl super::window_move::WindowAccess for MovableWindow {
    fn snapshot(&self) -> Result<super::window_move::Snapshot, String> {
        let bounds = element_rect(self.window.as_ptr(), &self.attributes)
            .ok_or_else(|| "cannot read the window's AX position and size".to_string())?;
        Ok(super::window_move::Snapshot {
            bounds,
            fullscreen: copy_bool_attribute(self.window.as_ptr(), &self.fullscreen),
        })
    }

    fn set_fullscreen(&self, enabled: bool) -> Result<(), WriteError> {
        let value = if enabled {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        self.set_attribute(&self.fullscreen, value.as_CFTypeRef())
    }

    fn set_position(&self, point: crate::api::geometry::Point) -> Result<(), WriteError> {
        let point = CGPoint::new(point.x, point.y);
        // SAFETY: the type tag describes exactly the CGPoint at this pointer.
        // AXValueCreate copies its contents; OwnedCf owns the resulting +1 object.
        let value = unsafe {
            OwnedCf::from_create_rule(AXValueCreate(AX_VALUE_CGPOINT, (&raw const point).cast()))
        }
        .ok_or_else(|| WriteError::Rejected("cannot create AX window position".to_string()))?;
        self.set_attribute(&self.attributes.position, value.as_ptr())
    }
}

pub fn ax_roles_for(semantic_roles: &[String]) -> Vec<String> {
    let mut roles = Vec::new();
    for role in semantic_roles {
        if let Some(native) = role.strip_prefix("ax:") {
            if !native.is_empty() && !roles.iter().any(|value| value == native) {
                roles.push(native.to_string());
            }
            continue;
        }
        let native = match role.as_str() {
            "button" => &["AXButton", "AXMenuButton", "AXPopUpButton"][..],
            "menu_button" => &["AXMenuButton"][..],
            "popup_button" => &["AXPopUpButton"][..],
            "link" => &["AXLink"][..],
            "checkbox" | "switch" => &["AXCheckBox"][..],
            "radio" => &["AXRadioButton"][..],
            "tab" => &["AXTabButton"][..],
            "menu_item" => &["AXMenuItem"][..],
            "text_field" => &["AXTextField", "AXTextArea", "AXSearchField"][..],
            "text_area" => &["AXTextArea"][..],
            "search_field" => &["AXSearchField"][..],
            "list_item" => &["AXRow", "AXOutlineRow"][..],
            "cell" => &["AXCell"][..],
            "tree_item" => &["AXDisclosureTriangle"][..],
            "combo_box" => &["AXComboBox"][..],
            "slider" => &["AXSlider"][..],
            "spinner" | "stepper" => &["AXIncrementor"][..],
            "scrollbar" => &["AXScrollBar"][..],
            "toolbar_button" => &["AXToolbarButton"][..],
            "image" => &["AXImage"][..],
            _ => &[][..],
        };
        for native_role in native {
            if !roles.iter().any(|value| value == native_role) {
                roles.push((*native_role).to_string());
            }
        }
    }
    roles
}

pub(crate) fn frontmost_pid() -> Option<libc::pid_t> {
    Some(
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .processIdentifier(),
    )
}

pub(crate) fn focused_window_bounds(pid: libc::pid_t) -> Result<Rect, String> {
    let application = AxApplication::new(pid)?;
    let attributes = AxAttributes::new();
    let window = match copy_attribute(application.as_ptr(), &attributes.focused_window) {
        Some(window) if is_ax_element(window.as_ptr()) => window,
        None => return Err("frontmost application has no focused window".into()),
        Some(_) => return Err("focused AX window has an unexpected Core Foundation type".into()),
    };
    element_rect(window.as_ptr(), &attributes)
        .ok_or_else(|| "focused window does not expose valid bounds".to_string())
}

pub(crate) fn scan_process_stream(
    pid: libc::pid_t,
    window_bounds: Rect,
    request: &UiScanRequest,
    deadline: Instant,
    is_current: impl Fn() -> bool,
    mut on_batch: impl FnMut(Vec<UiTarget>),
) -> Result<(), String> {
    if !is_current() || Instant::now() >= deadline {
        return Ok(());
    }
    let application = AxApplication::new(pid)?;

    let attributes = AxAttributes::new();
    let windows = copy_array_attribute(application.as_ptr(), &CFString::new("AXWindows"))
        .ok_or("selected application does not expose AXWindows")?;
    let root = windows.iter().find(|raw| {
        is_ax_element(**raw)
            && element_rect(**raw, &attributes).is_some_and(|rect| {
                (rect.x - window_bounds.x).abs() <= 2.0
                    && (rect.y - window_bounds.y).abs() <= 2.0
                    && (rect.width - window_bounds.width).abs() <= 2.0
                    && (rect.height - window_bounds.height).abs() <= 2.0
            })
    });
    let Some(root) = root else {
        return Ok(());
    };
    let Ok(scan_bounds) = visible_scan_bounds(Some(window_bounds), request.bounds) else {
        return Ok(());
    };
    let allowed_roles = ax_roles_for(&request.roles).into_iter().collect();
    let node_attributes = node_attribute_names(&attributes, request.visible_only);
    let mut scan = Scan {
        request,
        scan_bounds,
        attributes,
        node_attributes,
        attribute_reader: attribute_batch::Reader::default(),
        allowed_roles,
        deadline,
        on_batch: &mut on_batch,
        is_current: &is_current,
        seen: HashSet::with_capacity(128),
        occluders: Vec::new(),
    };
    scan.visit(root.cast(), 0);

    Ok(())
}

/// Inspect only on-screen AX windows, in Quartz front-to-back order, under one budget.
pub(crate) fn scan_screen_stream(
    request: &UiScanRequest,
    deadline: Instant,
    is_current: impl Fn() -> bool,
    mut on_batch: impl FnMut(Vec<UiTarget>),
    progress: impl Fn(crate::platform::common::scan_progress::Progress),
) -> Result<(), String> {
    use crate::platform::common::{scan_progress::Progress, window_visibility::matching_coverage};
    let bounds = request
        .bounds
        .ok_or("screen scan requires display bounds")?;
    let visible = window_manager::scan_windows()?;
    progress(Progress::Plan(
        visible
            .iter()
            .filter(|w| w.bounds.intersect(&bounds).is_some())
            .map(|w| matching_coverage(w.bounds))
            .collect(),
    ));
    let attributes = AxAttributes::new();
    let node_attributes = node_attribute_names(&attributes, request.visible_only);
    let mut scan = Scan {
        request,
        scan_bounds: Some(bounds),
        attributes,
        node_attributes,
        attribute_reader: attribute_batch::Reader::default(),
        allowed_roles: ax_roles_for(&request.roles).into_iter().collect(),
        deadline,
        on_batch: &mut on_batch,
        is_current: &is_current,
        seen: HashSet::with_capacity(128),
        occluders: Vec::new(),
    };
    let mut applications = HashMap::new();
    let minimized_key = CFString::new("AXMinimized");
    for item in visible
        .iter()
        .filter(|w| w.bounds.intersect(&bounds).is_some())
    {
        if !is_current() || Instant::now() >= deadline {
            break;
        }
        if crate::platform::common::window_visibility::visible_part(
            item.bounds,
            bounds,
            &scan.occluders,
            &request.screen_bounds,
        )
        .is_none()
        {
            progress(Progress::Complete(matching_coverage(item.bounds)));
            continue;
        }
        {
            let windows = applications.entry(item.pid).or_insert_with(|| {
                let mut windows = Vec::new();
                window_manager::application_windows(
                    item.pid,
                    NODE_TIMEOUT_SECONDS,
                    false,
                    &|| !is_current() || Instant::now() >= deadline,
                    |window| {
                        if !is_current()
                            || Instant::now() >= deadline
                            || copy_bool_attribute(window.window.as_ptr(), &minimized_key)
                                == Some(true)
                        {
                            return;
                        }
                        if let Some(frame) = element_rect(window.window.as_ptr(), &scan.attributes)
                        {
                            let title = copy_string_attribute(
                                window.window.as_ptr(),
                                &window.attributes.title,
                            )
                            .unwrap_or_default();
                            windows.push((window, frame, title));
                        }
                    },
                );
                windows
            });
            let candidates: smallvec::SmallVec<[(Rect, &str); 8]> = windows
                .iter()
                .map(|(_, frame, title)| (*frame, title.as_str()))
                .collect();
            let matching = crate::platform::common::window_visibility::visible_candidates(
                &candidates,
                item,
                &visible,
            );
            // An ambiguous AX identity must not expose a covered or off-Space window.
            if let [index] = matching.as_slice() {
                let (window, frame, _) = &windows[*index];
                scan.scan_bounds = frame.intersect(&bounds);
                scan.attribute_reader.reset();
                scan.visit(window.window.as_ptr(), 0);
            }
        }
        if item.pid as u32 != std::process::id() {
            scan.occluders.push(item.bounds);
        }
        progress(Progress::Complete(matching_coverage(item.bounds)));
    }
    Ok(())
}

struct Scan<'a> {
    request: &'a UiScanRequest,
    scan_bounds: Option<Rect>,
    attributes: AxAttributes,
    node_attributes: CFArray<CFString>,
    attribute_reader: attribute_batch::Reader,
    allowed_roles: HashSet<String>,
    deadline: Instant,
    on_batch: &'a mut dyn FnMut(Vec<UiTarget>),
    is_current: &'a dyn Fn() -> bool,
    seen: HashSet<(i64, i64, i64, i64)>,
    occluders: Vec<Rect>,
}

impl Scan<'_> {
    fn expired(&self) -> bool {
        Instant::now() >= self.deadline || !(self.is_current)()
    }

    fn visit(&mut self, element: AXUIElementRef, depth: u32) {
        if !is_ax_element(element)
            || (self.request.max_depth != 0 && depth > self.request.max_depth)
            || self.expired()
        {
            return;
        }

        let role = copy_string_attribute(element, &self.attributes.role).unwrap_or_default();
        if self.expired() {
            return;
        }
        let (prefer_visible_rows, prefer_visible_children) =
            visible_collection_preferences(&role, self.request.visible_only);
        let role_allowed =
            self.request.roles.is_empty() || self.allowed_roles.contains(role.as_str());
        let (rect, enabled, visible) = if role_allowed {
            let keys = &self.node_attributes;
            let deadline = self.deadline;
            let current = self.is_current;
            let Some(values) = self.attribute_reader.read(
                keys.len() as usize,
                || Instant::now() < deadline && current(),
                || copy_node_attributes(element, keys),
                |index| {
                    let key = keys.get(index as isize)?;
                    copy_attribute(element, &key).map(into_cf_type)
                },
            ) else {
                return;
            };
            decode_node_attributes(values, self.request.visible_only)
        } else {
            (None, false, false)
        };
        let rect = rect.and_then(|rect| {
            crate::platform::common::window_visibility::visible_part(
                rect,
                self.scan_bounds.unwrap_or(rect),
                &self.occluders,
                &self.request.screen_bounds,
            )
        });
        let in_bounds = rect.is_some_and(|rect| {
            self.scan_bounds
                .is_none_or(|bounds| bounds.contains(&rect.center()))
                && !self
                    .occluders
                    .iter()
                    .any(|bounds| bounds.contains(&rect.center()))
        });
        if self.expired() {
            return;
        }
        // Standard controls are mouse-addressable even when an application does
        // not expose AXPress. Action discovery is the slower fallback for custom
        // controls and only runs for an otherwise eligible candidate.
        let clickable = !self.request.clickable_only
            || intrinsically_clickable(&role)
            || (enabled && visible && in_bounds && has_actions(element));
        let semantic_role = semantic_role(&role);

        if self.expired() {
            return;
        }

        if enabled
            && visible
            && clickable
            && in_bounds
            && let Some(rect) = rect
        {
            let key = normalized_rect(rect);
            if self.seen.insert(key) {
                let name = accessible_name(element, &self.attributes, || !self.expired());
                if self.expired() {
                    return;
                }
                (self.on_batch)(vec![UiTarget {
                    details: None,
                    rect,
                    name,
                    role: semantic_role,
                }]);
            }
        }

        if self.expired() || self.request.max_depth != 0 && depth == self.request.max_depth {
            return;
        }
        let Some(children) = visible_children_for_role(
            element,
            prefer_visible_rows,
            prefer_visible_children,
            &self.attributes,
            || !self.expired(),
        ) else {
            return;
        };
        for child in &children {
            self.visit(*child, depth + 1);
            if self.expired() {
                break;
            }
        }
    }
}

/// Prefer the AX server's viewport-aware collections for scroll containers.
/// Unsupported attributes fall back to AXChildren, preserving compatibility
/// with custom controls while avoiding traversal of off-screen table rows in
/// standard AppKit lists.
fn visible_children_for_role(
    element: AXUIElementRef,
    prefer_visible_rows: bool,
    prefer_visible_children: bool,
    attributes: &AxAttributes,
    current: impl Fn() -> bool,
) -> Option<CFArray<*const c_void>> {
    let read = |name| {
        current()
            .then(|| copy_array_attribute(element, name))
            .flatten()
    };
    let visible = if prefer_visible_rows {
        read(&attributes.visible_children).or_else(|| read(&attributes.visible_rows))
    } else if prefer_visible_children {
        read(&attributes.visible_children)
    } else {
        None
    };
    visible.or_else(|| read(&attributes.children))
}

fn prefers_visible_rows(role: &str) -> bool {
    matches!(role, "AXTable" | "AXOutline" | "AXGrid")
}

fn prefers_visible_children(role: &str) -> bool {
    matches!(
        role,
        "AXScrollArea" | "AXList" | "AXBrowser" | "AXCollectionList" | "AXWebArea"
    )
}

fn visible_collection_preferences(role: &str, visible_only: bool) -> (bool, bool) {
    if visible_only {
        (prefers_visible_rows(role), prefers_visible_children(role))
    } else {
        (false, false)
    }
}

fn node_attribute_names(attributes: &AxAttributes, visible_only: bool) -> CFArray<CFString> {
    let names = [
        attributes.position.clone(),
        attributes.size.clone(),
        attributes.enabled.clone(),
        attributes.hidden.clone(),
    ];
    CFArray::from_CFTypes(&names[..if visible_only { 4 } else { 3 }])
}

fn copy_node_attributes(
    element: AXUIElementRef,
    names: &CFArray<CFString>,
) -> attribute_batch::Batch<CFType, 4> {
    let mut values = ptr::null();
    // SAFETY: the worker owns this live AX element and the immutable CFString
    // array. AXCopyMultipleAttributeOptions is UInt32, not CFOptionFlags. Zero
    // keeps errors in their individual slots instead of stopping the batch.
    // Adopt any returned +1 object even on failure, so it is always released.
    let (error, values) = unsafe {
        let error = AXUIElementCopyMultipleAttributeValues(
            element,
            names.as_concrete_TypeRef(),
            0,
            &mut values,
        );
        (error, OwnedCf::from_create_rule(values.cast()))
    };
    if error != AX_OK {
        return if attribute_batch::unsupported(error) {
            attribute_batch::Batch::Unsupported
        } else {
            attribute_batch::Batch::Failed
        };
    }
    let Some(values) = values.and_then(downcast_cf::<CFArray<*const c_void>>) else {
        return attribute_batch::Batch::Unsupported;
    };
    node_values_from_array(values, names.len())
}

fn node_values_from_array(
    values: CFArray<*const c_void>,
    count: isize,
) -> attribute_batch::Batch<CFType, 4> {
    if !(0..=4).contains(&count) || values.len() != count {
        // Never shift slots or silently omit fields in a malformed response.
        return attribute_batch::Batch::Unsupported;
    }
    attribute_batch::Batch::Values(std::array::from_fn(|index| {
        let value = *values.get(index as isize)?;
        if value.is_null() {
            return None;
        }
        // SAFETY: AX documents CF objects (including CFNull/AXValue errors) in
        // this checked array. Retain the borrowed non-null slot before the
        // array drops. Consumers type-check each field independently.
        Some(unsafe { CFType::wrap_under_get_rule(value) })
    }))
}

fn decode_node_attributes(
    [position, size, enabled, hidden]: [Option<CFType>; 4],
    visible_only: bool,
) -> (Option<Rect>, bool, bool) {
    // Release every native value before actions, names, publication or
    // recursion; no per-node attribute arrays accumulate on the stack.
    (
        position
            .as_ref()
            .zip(size.as_ref())
            .and_then(|(position, size)| rect_from_values(position, size)),
        enabled
            .and_then(|value| value.downcast_into::<CFBoolean>())
            .map(bool::from)
            .unwrap_or(true),
        !visible_only
            || !hidden
                .and_then(|value| value.downcast_into::<CFBoolean>())
                .map(bool::from)
                .unwrap_or(false),
    )
}

fn copy_array_attribute(
    element: AXUIElementRef,
    name: &CFString,
) -> Option<CFArray<*const c_void>> {
    downcast_cf(copy_attribute(element, name)?)
}

fn copy_attribute(element: AXUIElementRef, name: &CFString) -> Option<OwnedCf> {
    let mut value: CFTypeRef = ptr::null();
    // SAFETY: `element` and `name` are live for the call and `value` is a valid
    // out-pointer. Adopt any returned +1 object on both success and failure.
    let (error, value) = unsafe {
        let error = AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value);
        (error, OwnedCf::from_create_rule(value))
    };
    (error == AX_OK).then_some(value).flatten()
}

fn copy_string_attribute(element: AXUIElementRef, name: &CFString) -> Option<String> {
    let value = copy_attribute(element, name)?;
    let value = downcast_cf::<CFString>(value)?;
    Some(value.to_string())
}

fn copy_bool_attribute(element: AXUIElementRef, name: &CFString) -> Option<bool> {
    let value = copy_attribute(element, name)?;
    let value = downcast_cf::<CFBoolean>(value)?;
    Some(bool::from(value))
}

fn downcast_cf<T: ConcreteCFType>(value: OwnedCf) -> Option<T> {
    into_cf_type(value).downcast_into::<T>()
}

fn into_cf_type(value: OwnedCf) -> CFType {
    // SAFETY: OwnedCf holds one live +1 Core Foundation object. Ownership is
    // transferred into CFType exactly once, which releases it when dropped.
    unsafe { CFType::wrap_under_create_rule(value.into_raw().cast()) }
}

fn is_ax_element(value: *const c_void) -> bool {
    !value.is_null()
        // SAFETY: callers pass either a live Core Foundation object obtained
        // from AX or null, rejected above. Both type-id functions are pure.
        && unsafe { CFGetTypeID(value) == AXUIElementGetTypeID() }
}

fn element_rect(element: AXUIElementRef, attributes: &AxAttributes) -> Option<Rect> {
    let position = into_cf_type(copy_attribute(element, &attributes.position)?);
    let size = into_cf_type(copy_attribute(element, &attributes.size)?);
    rect_from_values(&position, &size)
}

fn rect_from_values(position: &CFType, size: &CFType) -> Option<Rect> {
    let mut point = CGPoint::new(0.0, 0.0);
    let mut dimensions = CGSize::new(0.0, 0.0);
    // SAFETY: both +1 objects are live Core Foundation values. CFGetTypeID
    // first proves they are AXValue objects before AXValueGetType inspects the
    // payload, and AXValueGetValue writes into correctly sized outputs.
    let (point_ok, size_ok) = unsafe {
        if CFGetTypeID(position.as_CFTypeRef()) != AXValueGetTypeID()
            || CFGetTypeID(size.as_CFTypeRef()) != AXValueGetTypeID()
            || AXValueGetType(position.as_CFTypeRef()) != AX_VALUE_CGPOINT
            || AXValueGetType(size.as_CFTypeRef()) != AX_VALUE_CGSIZE
        {
            return None;
        }
        (
            AXValueGetValue(
                position.as_CFTypeRef(),
                AX_VALUE_CGPOINT,
                (&raw mut point).cast(),
            ),
            AXValueGetValue(
                size.as_CFTypeRef(),
                AX_VALUE_CGSIZE,
                (&raw mut dimensions).cast(),
            ),
        )
    };
    let rect = Rect::new(point.x, point.y, dimensions.width, dimensions.height);
    (point_ok && size_ok && !rect.is_empty() && rect.width.is_finite() && rect.height.is_finite())
        .then_some(rect)
}

fn intrinsically_clickable(role: &str) -> bool {
    matches!(
        role,
        "AXButton"
            | "AXMenuButton"
            | "AXPopUpButton"
            | "AXToolbarButton"
            | "AXLink"
            | "AXCheckBox"
            | "AXRadioButton"
            | "AXTabButton"
            | "AXMenuItem"
            | "AXTextField"
            | "AXTextArea"
            | "AXSearchField"
            | "AXRow"
            | "AXOutlineRow"
            | "AXCell"
            | "AXDisclosureTriangle"
            | "AXComboBox"
            | "AXSlider"
            | "AXIncrementor"
            | "AXScrollBar"
    )
}

fn has_actions(element: AXUIElementRef) -> bool {
    let mut actions = ptr::null();
    // SAFETY: `element` is live and `actions` is a valid out-pointer receiving
    // a +1 array only when AX_OK is returned.
    if unsafe { AXUIElementCopyActionNames(element, &mut actions) } != AX_OK || actions.is_null() {
        return false;
    }
    // SAFETY: AX CopyActionNames returned a non-null +1 array above.
    let actions = unsafe { OwnedCf::from_create_rule(actions) };
    let Some(actions) = actions else {
        return false;
    };
    let Some(actions) = downcast_cf::<CFArray<*const c_void>>(actions) else {
        return false;
    };
    !actions.is_empty()
}

fn accessible_name(
    element: AXUIElementRef,
    attributes: &AxAttributes,
    current: impl Fn() -> bool,
) -> String {
    [&attributes.title, &attributes.description, &attributes.help]
        .into_iter()
        .find_map(|attribute| {
            current()
                .then(|| copy_string_attribute(element, attribute))
                .flatten()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_default()
}

fn semantic_role(native_role: &str) -> SemanticRole {
    match native_role {
        "AXButton" | "AXMenuButton" | "AXPopUpButton" | "AXToolbarButton" => SemanticRole::Button,
        "AXLink" => SemanticRole::Link,
        "AXCheckBox" => SemanticRole::Checkbox,
        "AXRadioButton" => SemanticRole::Radio,
        "AXTabButton" => SemanticRole::Tab,
        "AXMenuItem" => SemanticRole::MenuItem,
        "AXTextField" | "AXTextArea" | "AXSearchField" => SemanticRole::TextField,
        "AXRow" | "AXOutlineRow" => SemanticRole::ListItem,
        "AXDisclosureTriangle" => SemanticRole::TreeItem,
        "AXComboBox" => SemanticRole::ComboBox,
        "AXSlider" => SemanticRole::Slider,
        "AXIncrementor" => SemanticRole::Spinner,
        "AXScrollBar" => SemanticRole::Scrollbar,
        "AXImage" => SemanticRole::Image,
        "AXStaticText" => SemanticRole::StaticText,
        "AXCell" => SemanticRole::Cell,
        _ => SemanticRole::Control,
    }
}

fn normalized_rect(rect: Rect) -> (i64, i64, i64, i64) {
    (
        (rect.x * 2.0).round() as i64,
        (rect.y * 2.0).round() as i64,
        (rect.width * 2.0).round() as i64,
        (rect.height * 2.0).round() as i64,
    )
}

/// Restrict AX targets to the visible portion of the focused window on the
/// requested display. `Err(())` means the two regions do not overlap at all.
fn visible_scan_bounds(window: Option<Rect>, requested: Option<Rect>) -> Result<Option<Rect>, ()> {
    match (window, requested) {
        (Some(window), Some(requested)) => window.intersect(&requested).map(Some).ok_or(()),
        (Some(window), None) => Ok(Some(window)),
        (None, requested) => Ok(requested),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn native_attribute_values() -> [CFType; 4] {
        let point = CGPoint::new(-1920.0, 36.0);
        let size = CGSize::new(120.0, 28.0);
        let error: c_int = -25205;
        // SAFETY: each tag matches the exact scalar ABI at its live pointer.
        // Create-rule values are adopted once; the process-lifetime CFNull is
        // retained under the get rule. No AX provider or permission is needed.
        unsafe {
            [
                into_cf_type(
                    OwnedCf::from_create_rule(AXValueCreate(
                        AX_VALUE_CGPOINT,
                        (&raw const point).cast(),
                    ))
                    .unwrap(),
                ),
                into_cf_type(
                    OwnedCf::from_create_rule(AXValueCreate(
                        AX_VALUE_CGSIZE,
                        (&raw const size).cast(),
                    ))
                    .unwrap(),
                ),
                into_cf_type(
                    OwnedCf::from_create_rule(AXValueCreate(5, (&raw const error).cast())).unwrap(),
                ),
                CFType::wrap_under_get_rule(core_foundation::base::kCFNull),
            ]
        }
    }

    #[test]
    fn native_batch_decoding_preserves_geometry_and_independent_missing_flags() {
        let [position, size, error, null] = native_attribute_values();
        let expected = Rect::new(-1920.0, 36.0, 120.0, 28.0);
        let counts = (position.retain_count(), size.retain_count());
        for _ in 0..100 {
            let values = CFArray::from_CFTypes(&[
                position.clone(),
                size.clone(),
                error.clone(),
                null.clone(),
            ]);
            let attribute_batch::Batch::Values(values) =
                node_values_from_array(values.into_untyped(), 4)
            else {
                panic!("a complete native array must preserve all slots");
            };
            assert_eq!(
                decode_node_attributes(values, true),
                (Some(expected), true, true)
            );
        }
        assert_eq!((position.retain_count(), size.retain_count()), counts);
        assert_eq!(
            decode_node_attributes(
                [
                    Some(position.clone()),
                    Some(size.clone()),
                    Some(CFBoolean::false_value().into_CFType()),
                    Some(CFBoolean::true_value().into_CFType())
                ],
                true
            ),
            (Some(expected), false, false)
        );
        assert_eq!(
            decode_node_attributes(
                [
                    Some(position),
                    Some(size),
                    None,
                    Some(CFBoolean::true_value().into_CFType())
                ],
                false
            ),
            (Some(expected), true, true)
        );
        assert!(rect_from_values(&error, &null).is_none());
        assert!(
            rect_from_values(
                &CFString::new("not a position").into_CFType(),
                &CFBoolean::true_value().into_CFType()
            )
            .is_none()
        );
    }

    #[test]
    fn native_batch_shape_and_request_fields_are_checked() {
        let attributes = AxAttributes::new();
        for visible_only in [false, true] {
            let names = node_attribute_names(&attributes, visible_only);
            let expected: &[&str] = if visible_only {
                &["AXPosition", "AXSize", "AXEnabled", "AXHidden"]
            } else {
                &["AXPosition", "AXSize", "AXEnabled"]
            };
            assert_eq!(
                names
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let values = native_attribute_values();
        for count in [3, 5] {
            assert!(matches!(
                node_values_from_array(CFArray::from_CFTypes(&values).into_untyped(), count),
                attribute_batch::Batch::Unsupported
            ));
        }
        let attribute_batch::Batch::Values(values) =
            node_values_from_array(CFArray::from_CFTypes(&values[..3]).into_untyped(), 3)
        else {
            panic!("three requested fields must be valid");
        };
        assert!(values[3].is_none());
    }

    #[test]
    #[ignore = "requires macOS Accessibility permission and a stable frontmost window"]
    fn native_ax_batch_matches_individual_reads_and_cancels_after_first_target() {
        use crate::api::command::{UiScanScope, UiScanStrategy, VisionOptions};
        use std::cell::Cell;

        let application = AxApplication::new(frontmost_pid().expect("frontmost app")).unwrap();
        let attributes = AxAttributes::new();
        let window = copy_attribute(application.as_ptr(), &attributes.focused_window).unwrap();
        assert!(is_ax_element(window.as_ptr()));
        let bounds = element_rect(window.as_ptr(), &attributes).unwrap();
        let request = UiScanRequest {
            id: 1,
            scope: UiScanScope::Window,
            bounds: Some(bounds),
            screen_bounds: vec![bounds],
            roles: Vec::new(),
            max_depth: 0,
            timeout_ms: 10_000,
            visible_only: true,
            clickable_only: true,
            strategy: UiScanStrategy::AxTree,
            vision: VisionOptions::default(),
            app: None,
        };
        let read = |reader, cancel_after_first| {
            let current = Cell::new(true);
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut targets = Vec::new();
            {
                let mut publish = |batch: Vec<UiTarget>| {
                    assert_eq!(
                        batch.len(),
                        1,
                        "AX targets must remain streamed individually"
                    );
                    targets.extend(batch);
                    if cancel_after_first {
                        current.set(false);
                    }
                };
                let attributes = AxAttributes::new();
                let node_attributes = node_attribute_names(&attributes, true);
                let mut scan = Scan {
                    request: &request,
                    scan_bounds: Some(bounds),
                    attributes,
                    node_attributes,
                    attribute_reader: reader,
                    allowed_roles: HashSet::new(),
                    deadline,
                    on_batch: &mut publish,
                    is_current: &|| current.get(),
                    seen: HashSet::new(),
                    occluders: Vec::new(),
                };
                scan.visit(window.as_ptr(), 0);
                assert!(
                    Instant::now() < deadline,
                    "incomplete scan cannot establish equivalence"
                );
            }
            targets
        };
        let before = read(attribute_batch::Reader::individual(), false);
        assert!(
            !before.is_empty(),
            "the stable window must expose usable targets"
        );
        let batched = read(attribute_batch::Reader::default(), false);
        let after = read(attribute_batch::Reader::individual(), false);
        assert_eq!(before, after, "the window changed during the comparison");
        assert_eq!(
            batched, before,
            "batching must preserve target values and order"
        );
        let cancelled = read(attribute_batch::Reader::default(), true);
        assert_eq!(cancelled, before[..1]);
    }

    #[test]
    fn semantic_roles_expand_without_duplicates() {
        let roles = ax_roles_for(&[
            "button".into(),
            "link".into(),
            "cell".into(),
            "ax:AXCustomControl".into(),
            "button".into(),
        ]);
        assert!(roles.contains(&"AXButton".into()));
        assert!(roles.contains(&"AXLink".into()));
        assert!(roles.contains(&"AXCell".into()));
        assert!(roles.contains(&"AXCustomControl".into()));
        let unique: BTreeSet<_> = roles.iter().collect();
        assert_eq!(unique.len(), roles.len());
    }

    #[test]
    fn standard_ax_controls_do_not_require_action_discovery() {
        assert!(intrinsically_clickable("AXButton"));
        assert!(intrinsically_clickable("AXTextField"));
        assert!(intrinsically_clickable("AXCell"));
        assert!(!intrinsically_clickable("AXGroup"));
    }

    #[test]
    fn scroll_containers_prefer_viewport_aware_ax_collections() {
        assert!(prefers_visible_rows("AXTable"));
        assert!(prefers_visible_rows("AXOutline"));
        assert!(!prefers_visible_rows("AXGroup"));
        assert!(prefers_visible_children("AXScrollArea"));
        assert!(prefers_visible_children("AXList"));
        assert!(!prefers_visible_children("AXGroup"));
        assert_eq!(
            visible_collection_preferences("AXTable", false),
            (false, false)
        );
    }

    #[test]
    fn normalized_rect_deduplicates_subpixel_noise() {
        let first = normalized_rect(Rect::new(10.01, 20.01, 30.01, 40.01));
        let second = normalized_rect(Rect::new(10.02, 20.02, 30.02, 40.02));
        assert_eq!(first, second);
    }

    #[test]
    fn visible_scan_bounds_clip_ax_targets_to_the_focused_window() {
        let window = Rect::new(100.0, 100.0, 800.0, 600.0);
        let display = Rect::new(0.0, 0.0, 600.0, 500.0);
        assert_eq!(
            visible_scan_bounds(Some(window), Some(display)),
            Ok(Some(Rect::new(100.0, 100.0, 500.0, 400.0)))
        );
        assert_eq!(visible_scan_bounds(Some(window), None), Ok(Some(window)));
        assert_eq!(
            visible_scan_bounds(Some(window), Some(Rect::new(0.0, 0.0, 50.0, 50.0))),
            Err(())
        );
    }
}
