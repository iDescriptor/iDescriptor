// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(target_os = "macos")]
mod appkit {
    use std::{cell::Cell, ffi::c_void};

    use objc2::{
        AnyThread, ClassType, MainThreadMarker, MainThreadOnly, define_class,
        ffi::{
            OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject,
        },
        msg_send,
        rc::{Allocated, Retained},
        runtime::AnyObject,
        sel,
    };
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSColor, NSToolbar, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowButton,
        NSWindowDidResizeNotification, NSWindowOrderingMode, NSWindowStyleMask,
        NSWindowTitleVisibility,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSPoint, NSString};

    struct TrafficLightInsetIvars {
        window: *mut NSWindow,
        position: Cell<NSPoint>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "IDTrafficLightInsetObserver"]
        #[ivars = TrafficLightInsetIvars]
        struct TrafficLightInsetObserver;

        impl TrafficLightInsetObserver {
            #[unsafe(method_id(initWithWindow:position:))]
            fn init_with_window(
                this: Allocated<Self>,
                window: &NSWindow,
                position: NSPoint,
            ) -> Retained<Self> {
                let this = this.set_ivars(TrafficLightInsetIvars {
                    window: window as *const NSWindow as *mut NSWindow,
                    position: Cell::new(position),
                });
                let this: Retained<Self> = unsafe { msg_send![super(this), init] };
                unsafe {
                    NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                        &this,
                        sel!(windowDidResize:),
                        Some(NSWindowDidResizeNotification),
                        Some(window),
                    );
                }
                this
            }

            #[unsafe(method(setPosition:))]
            fn set_position(&self, position: NSPoint) {
                self.ivars().position.set(position);
                unsafe { set_traffic_light_inset(&*self.ivars().window, position) };
            }

            #[unsafe(method(windowDidResize:))]
            fn window_did_resize(&self, _notification: &NSNotification) {
                unsafe {
                    set_traffic_light_inset(&*self.ivars().window, self.ivars().position.get())
                };
            }
        }
    );

    impl Drop for TrafficLightInsetObserver {
        fn drop(&mut self) {
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver(self);
            }
        }
    }

    define_class!(
        #[unsafe(super(NSVisualEffectView))]
        #[name = "IDSidebarVisualEffectView"]
        struct SidebarVisualEffectView;

        impl SidebarVisualEffectView {
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
                None
            }
        }
    );

    static mut TRAFFIC_LIGHT_OBSERVER_KEY: u8 = 0;
    static mut VISUAL_EFFECT_KEY: u8 = 0;

    unsafe fn window_from_view_ptr(view_ptr: *mut c_void) -> Option<Retained<NSWindow>> {
        if view_ptr.is_null() {
            log::warn!("Native macOS view pointer is null");
            return None;
        }

        let native_view = unsafe { &*(view_ptr.cast::<NSView>()) };
        let window = native_view.window();
        if window.is_none() {
            log::warn!("Native macOS window is null");
        }
        window
    }

    unsafe fn set_traffic_light_inset(window: &NSWindow, position: NSPoint) {
        let Some(close_button) = window.standardWindowButton(NSWindowButton::CloseButton) else {
            return;
        };
        let Some(minimize_button) = window.standardWindowButton(NSWindowButton::MiniaturizeButton)
        else {
            return;
        };
        let Some(zoom_button) = window.standardWindowButton(NSWindowButton::ZoomButton) else {
            return;
        };
        let Some(title_bar) = close_button.superview().and_then(|view| view.superview()) else {
            return;
        };

        let close_frame = close_button.frame();
        let title_bar_height = close_frame.size.height + position.y;
        let mut title_bar_frame = title_bar.frame();
        title_bar_frame.size.height = title_bar_height;
        title_bar_frame.origin.y = window.frame().size.height - title_bar_height;
        title_bar.setFrame(title_bar_frame);

        let spacing = minimize_button.frame().origin.x - close_frame.origin.x;
        for (index, button) in [close_button, minimize_button, zoom_button]
            .into_iter()
            .enumerate()
        {
            let mut origin = button.frame().origin;
            origin.x = position.x + index as f64 * spacing;
            button.setFrameOrigin(origin);
        }
    }

    unsafe fn keep_traffic_light_inset(window: &NSWindow, position: NSPoint) {
        let key = (&raw const TRAFFIC_LIGHT_OBSERVER_KEY).cast::<c_void>();
        let existing =
            unsafe { objc_getAssociatedObject(window as *const NSWindow as *const AnyObject, key) };
        if !existing.is_null() {
            unsafe { &*existing.cast::<TrafficLightInsetObserver>() }.set_position(position);
            return;
        }

        let observer = TrafficLightInsetObserver::init_with_window(
            TrafficLightInsetObserver::alloc(),
            window,
            position,
        );
        unsafe {
            objc_setAssociatedObject(
                window as *const NSWindow as *mut AnyObject,
                key,
                Retained::as_ptr(&observer) as *mut AnyObject,
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
            set_traffic_light_inset(window, position);
        }
    }

    unsafe fn install_sidebar_material(
        window: &NSWindow,
        native_view: &NSView,
        mtm: MainThreadMarker,
    ) {
        let key = (&raw const VISUAL_EFFECT_KEY).cast::<c_void>();
        if !unsafe { objc_getAssociatedObject(window as *const NSWindow as *const AnyObject, key) }
            .is_null()
        {
            return;
        }

        let Some(container_view) = native_view.superview() else {
            log::warn!("Failed to install sidebar material: native view has no superview");
            return;
        };

        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));

        let effect_view: Retained<SidebarVisualEffectView> =
            unsafe { msg_send![SidebarVisualEffectView::alloc(mtm), init] };
        effect_view.setFrame(native_view.frame());
        effect_view.setMaterial(NSVisualEffectMaterial::Sidebar);
        effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect_view.setState(NSVisualEffectState::FollowsWindowActiveState);
        effect_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Keep Qt's native view exactly where Qt installed it. Reparenting this
        // view disrupts Qt's native responder and pointer-tracking setup.
        unsafe {
            container_view.addSubview_positioned_relativeTo(
                &effect_view,
                NSWindowOrderingMode::Below,
                Some(native_view),
            );
            objc_setAssociatedObject(
                window as *const NSWindow as *mut AnyObject,
                key,
                Retained::as_ptr(&effect_view) as *mut AnyObject,
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        }
    }

    pub unsafe fn setup_tool_frame(view_ptr: *mut c_void, mtm: MainThreadMarker) {
        let Some(window) = (unsafe { window_from_view_ptr(view_ptr) }) else {
            log::warn!("Failed to set up the macOS tool window frame");
            return;
        };

        // TODO: remove the fullscreen button. Changing the style mask here
        // currently breaks other window styling behavior.
        let identifier = NSString::from_str("HiddenInsetToolbar");
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &identifier);
        toolbar.setShowsBaselineSeparator(false);
        window.setToolbar(Some(&toolbar));

        // TODO: theming
        window.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.95, 1.0)));
    }

    pub unsafe fn setup_main_window(view_ptr: *mut c_void, mtm: MainThreadMarker) {
        if view_ptr.is_null() {
            log::warn!("Native macOS view pointer is null");
            return;
        }
        let native_view = unsafe { &*(view_ptr.cast::<NSView>()) };
        let Some(window) = (unsafe { window_from_view_ptr(view_ptr) }) else {
            log::warn!("Failed to set up the macOS main window");
            return;
        };

        window.setStyleMask(
            window.styleMask() | NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView,
        );
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setMovableByWindowBackground(false);
        unsafe {
            install_sidebar_material(&window, native_view, mtm);
            keep_traffic_light_inset(&window, NSPoint::new(20.0, 20.0));
        }
        window.center();
    }

    pub fn main_thread_marker() -> Option<MainThreadMarker> {
        MainThreadMarker::new()
    }
}

#[cfg(target_os = "macos")]
pub fn apply_tool_frame(win_id: usize) {
    if win_id == 0 {
        log::warn!("Skipping macOS tool window setup: QWindow::winId() returned 0");
        return;
    }
    let Some(mtm) = appkit::main_thread_marker() else {
        log::error!("macOS tool window setup must run on the main thread");
        return;
    };

    log::debug!("Applying macOS tool window setup: {}", win_id);
    unsafe { appkit::setup_tool_frame(win_id as *mut std::ffi::c_void, mtm) };
}

#[cfg(not(target_os = "macos"))]
pub fn apply_tool_frame(_win_id: usize) {}

#[cfg(target_os = "macos")]
pub fn apply_main_window(win_id: usize) {
    if win_id == 0 {
        log::warn!("Skipping macOS main window setup: QWindow::winId() returned 0");
        return;
    }
    let Some(mtm) = appkit::main_thread_marker() else {
        log::error!("macOS main window setup must run on the main thread");
        return;
    };

    log::debug!("Applying macOS main window setup: {}", win_id);
    unsafe { appkit::setup_main_window(win_id as *mut std::ffi::c_void, mtm) };
}

#[cfg(not(target_os = "macos"))]
pub fn apply_main_window(_win_id: usize) {}
