use super::{observe_workspace, safety::Guard, timer};
use crate::{
    breathing::Entrance,
    config::{MAX_CONFIG_BYTES, Settings},
    process::boot_time,
    reminder, render,
};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, runtime::ProtocolObject, sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSTimer,
};
use std::{
    cell::{Cell, RefCell},
    io::{BufRead, BufReader, Read, Write},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

struct Drawing {
    bitmap: Retained<NSBitmapImageRep>,
    quote: Vec<u8>,
    size: (usize, usize),
    scale: f32,
    font: Rc<fontdue::Font>,
    entrance: Rc<Entrance>,
    started: Rc<Cell<Duration>>,
}

define_class!(
    // SAFETY: NSView has no additional subclassing requirements. All drawing stays on the main thread.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Drawing>]
    struct ReminderView;
    unsafe impl NSObjectProtocol for ReminderView {}
    impl ReminderView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_keyboard(&self) -> bool { true }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if event.keyCode() == 53 { stop(self.mtm()); }
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}
        #[unsafe(method(drawRect:))]
        fn draw(&self, _rect: NSRect) {
            let drawing = self.ivars().borrow_mut();
            let elapsed = boot_time().saturating_sub(drawing.started.get());
            let frame = drawing.entrance.frame(elapsed);
            // SAFETY: the owned nonplanar bitmap has exactly width*height*4 bytes;
            // main-thread RefCell access excludes any concurrent writes.
            let canvas = unsafe { std::slice::from_raw_parts_mut(
                drawing.bitmap.bitmapData(), drawing.size.0 * drawing.size.1 * 4,
            ) };
            if frame.active && frame.reflection == 0.0 {
                render::render_breathing(canvas, drawing.size, &drawing.font, drawing.scale,
                    render::BreathingView { pattern: &drawing.entrance.pattern, frame, full: true });
            } else {
                render::render_reveal(canvas, &drawing.quote, frame.reflection);
            }
            // Copy, rather than source-over, avoids accumulating opacity across frames.
            unsafe { drawing.bitmap.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                self.bounds(), NSRect::ZERO, NSCompositingOperation::Copy, 1.0, true, None,
            ); }
        }
    }
);

define_class!(
    // SAFETY: NSWindow has no additional subclassing requirements; ownership is retained explicitly.
    #[unsafe(super = NSWindow)]
    #[thread_kind = MainThreadOnly]
    struct ReminderWindow;
    unsafe impl NSObjectProtocol for ReminderWindow {}
    impl ReminderWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }
    }
);

struct OverlayState {
    windows: RefCell<Vec<Retained<ReminderWindow>>>,
    views: RefCell<Vec<Retained<ReminderView>>>,
    guard: Guard,
    started: Rc<Cell<Duration>>,
    visible_for: Duration,
    timer: RefCell<Option<Retained<NSTimer>>>,
    previous: Option<Retained<NSRunningApplication>>,
    restore_focus: Cell<bool>,
    shown: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no custom Drop or cross-thread access.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = OverlayState]
    struct OverlayDelegate;
    unsafe impl NSObjectProtocol for OverlayDelegate {}
    unsafe impl NSApplicationDelegate for OverlayDelegate {
        #[unsafe(method(applicationDidResignActive:))]
        fn resigned(&self, _notification: &NSNotification) {
            self.ivars().restore_focus.set(false);
            stop(self.mtm());
        }
    }
    impl OverlayDelegate {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            self.update();
        }
        #[unsafe(method(desktopChanged:))]
        fn desktop_changed(&self, _notification: &NSNotification) {
            self.ivars().restore_focus.set(false);
            stop(self.mtm());
        }
        #[unsafe(method(screensChanged:))]
        fn screens_changed(&self, _notification: &NSNotification) { stop(self.mtm()); }
    }
);

fn stop(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);
    // A timer/notification is not an NSEvent. Wake nextEvent so run() actually exits.
    if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined, NSPoint::ZERO, NSEventModifierFlags::empty(), 0.0, 0, None, 0, 0, 0,
    ) { app.postEvent_atStart(&event, true); }
}

impl OverlayDelegate {
    fn update(&self) {
        let state = self.ivars();
        let app = NSApplication::sharedApplication(self.mtm());
        let now = boot_time();
        if reminder::blocked(state.guard.latest.lock().unwrap().as_ref(), now).is_some() {
            state.restore_focus.set(false);
            stop(self.mtm());
            return;
        }
        if !state.shown.get() {
            if !app.isActive() {
                return;
            }
            let windows = state.windows.borrow();
            let Some(first) = windows.first() else {
                stop(self.mtm());
                return;
            };
            first.makeKeyAndOrderFront(None);
            if !first.isKeyWindow() {
                stop(self.mtm());
                return;
            }
            for window in windows.iter() {
                window.orderFrontRegardless();
            }
            if windows.iter().any(|window| !window.isVisible()) {
                stop(self.mtm());
                return;
            }
            state.started.set(now);
            state.shown.set(true);
            if std::io::stdout()
                .write_all(b"R")
                .and_then(|()| std::io::stdout().flush())
                .is_err()
            {
                stop(self.mtm());
            }
            return;
        }
        if !app.isActive()
            || !state
                .windows
                .borrow()
                .iter()
                .any(|window| window.isKeyWindow())
        {
            state.restore_focus.set(false);
            stop(self.mtm());
        } else if now.saturating_sub(state.started.get()) >= state.visible_for {
            stop(self.mtm());
        } else {
            for view in state.views.borrow().iter() {
                view.setNeedsDisplay(true);
            }
        }
    }
}

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let started = boot_time();
    let heartbeat = Arc::new(AtomicU64::new(started.as_nanos() as u64));
    let deadline = Arc::new(AtomicU64::new(
        (started + Duration::from_secs(3)).as_nanos() as u64,
    ));
    let (last, end) = (heartbeat.clone(), deadline.clone());
    // This thread is independent of AppKit, rendering, platform calls, and stdin.
    std::thread::spawn(move || {
        loop {
            let now = boot_time();
            let heartbeat = Duration::from_nanos(last.load(Ordering::Relaxed));
            if now.as_nanos() >= u128::from(end.load(Ordering::Relaxed))
                || reminder::watchdog_expired(now, started, heartbeat, Duration::MAX)
            {
                // SAFETY: immediate process exit releases WindowServer surfaces even if AppKit is stalled.
                unsafe {
                    libc::_exit(1);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let mut reader = BufReader::new(std::io::stdin());
    let mut payload = Vec::new();
    (&mut reader)
        .take(MAX_CONFIG_BYTES + 1)
        .read_until(b'\n', &mut payload)?;
    if payload.len() as u64 > MAX_CONFIG_BYTES || payload.last() != Some(&b'\n') {
        return Err("Missing or oversized supervised reminder settings.".into());
    }
    let settings: Settings = serde_json::from_slice(&payload)?;
    settings.validate()?;
    deadline.store(
        (started + Duration::from_secs(settings.helper_lifetime_secs())).as_nanos() as u64,
        Ordering::Relaxed,
    );
    std::thread::spawn(move || {
        let mut bytes = [0; 128];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => unsafe { libc::_exit(1) },
                Ok(_) => heartbeat.store(boot_time().as_nanos() as u64, Ordering::Relaxed),
            }
        }
    });
    let guard = Guard::start(settings.idle_secs);
    loop {
        let sample = guard.latest.lock().unwrap().clone();
        if sample.is_some() {
            if let Some(reason) = reminder::blocked(sample.as_ref(), boot_time()) {
                return Err(reason.into());
            }
            break;
        }
        if boot_time().saturating_sub(started) > Duration::from_secs(2) {
            return Err("Desktop safety startup timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mtm = MainThreadMarker::new().ok_or("AppKit requires the main thread.")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let previous = NSWorkspace::sharedWorkspace().frontmostApplication();
    let shared_start = Rc::new(Cell::new(boot_time()));
    let entrance = Rc::new(Entrance::new(settings.breathing.clone(), seed()?));
    let font = Rc::new(render::default_font()?);
    let state = OverlayState {
        windows: RefCell::new(Vec::new()),
        views: RefCell::new(Vec::new()),
        guard,
        started: shared_start.clone(),
        visible_for: settings.breathing.duration() + Duration::from_secs(settings.duration_secs),
        timer: RefCell::new(None),
        previous,
        restore_focus: Cell::new(true),
        shown: Cell::new(false),
    };
    let allocated = OverlayDelegate::alloc(mtm).set_ivars(state);
    let delegate: Retained<OverlayDelegate> = unsafe { msg_send![super(allocated), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return Err("No displays available.".into());
    }
    let mut pixels = 0usize;
    for screen in screens {
        let frame = screen.frame();
        let scale = screen.backingScaleFactor();
        let size = reminder::pixel_size(frame.size.width, frame.size.height, scale)?;
        pixels += size.0 * size.1;
        if pixels > reminder::MAX_TOTAL_PIXELS {
            return Err("Combined displays exceed the reminder memory limit.".into());
        }
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(), std::ptr::null_mut(), size.0 as isize, size.1 as isize,
                8, 4, true, false, NSDeviceRGBColorSpace,
                NSBitmapFormat::AlphaFirst | NSBitmapFormat::ThirtyTwoBitLittleEndian,
                (size.0 * 4) as isize, 32,
            )
        }.ok_or("Could not allocate reminder bitmap.")?;
        if bitmap.bitmapData().is_null() || bitmap.bytesPerRow() != (size.0 * 4) as isize {
            return Err("Invalid reminder bitmap layout.".into());
        }
        let mut quote = Vec::new();
        quote.try_reserve_exact(size.0 * size.1 * 4)?;
        quote.resize(size.0 * size.1 * 4, 0);
        render::render_overlay(
            &mut quote,
            size,
            &font,
            scale as f32,
            render::OverlayView {
                quote: &settings.quotes[0],
                text_size: settings.text_size as f32 * scale as f32,
                reflections_enabled: false,
                charts: &Default::default(),
                focus: &Default::default(),
            },
        );
        let drawing = Drawing {
            bitmap,
            quote,
            size,
            scale: scale as f32,
            font: font.clone(),
            entrance: entrance.clone(),
            started: shared_start.clone(),
        };
        let allocated = ReminderView::alloc(mtm).set_ivars(RefCell::new(drawing));
        let view: Retained<ReminderView> = unsafe {
            msg_send![super(allocated), initWithFrame: NSRect::new(NSPoint::ZERO, frame.size)]
        };
        let allocated = ReminderWindow::alloc(mtm).set_ivars(());
        let window: Retained<ReminderWindow> = unsafe {
            msg_send![super(allocated), initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered, defer: false]
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setOpaque(false);
        window.setHasShadow(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        // Never use screen-saver/security levels or capture the physical displays.
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        window.setContentView(Some(&view));
        if !window.makeFirstResponder(Some(&view)) {
            return Err("Reminder keyboard focus could not be prepared.".into());
        }
        delegate.ivars().windows.borrow_mut().push(window);
        delegate.ivars().views.borrow_mut().push(view);
    }
    observe_workspace(&delegate, sel!(desktopChanged:));
    app.finishLaunching();
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &delegate,
            sel!(screensChanged:),
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
        );
    }
    *delegate.ivars().timer.borrow_mut() = Some(timer(&delegate, sel!(tick:), 1.0 / 30.0));
    app.run();
    if let Some(timer) = delegate.ivars().timer.borrow_mut().take() {
        timer.invalidate();
    }
    // SAFETY: this is the same retained observer registered above.
    unsafe {
        NSNotificationCenter::defaultCenter().removeObserver(&delegate);
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .removeObserver(&delegate);
    }
    for window in delegate.ivars().windows.borrow().iter() {
        window.orderOut(None);
    }
    if delegate.ivars().restore_focus.get()
        && app.isActive()
        && reminder::blocked(
            delegate.ivars().guard.latest.lock().unwrap().as_ref(),
            boot_time(),
        )
        .is_none()
        && let Some(previous) = &delegate.ivars().previous
    {
        #[allow(deprecated)]
        previous.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
    Ok(())
}

fn seed() -> Result<u64, std::io::Error> {
    let mut bytes = [0; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u64::from_ne_bytes(bytes))
}
