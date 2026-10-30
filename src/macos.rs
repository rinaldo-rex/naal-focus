mod engine;
mod overlay;
mod safety;
mod settings;

use crate::{
    config::{self, Settings},
    private_fs,
};
use engine::{Engine, Request};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes,
    NSSize, NSString, NSTimer,
};
use settings::SettingsWindow;
use std::{
    cell::{Cell, OnceCell, RefCell},
    path::Path,
};

const HELP: &str = "Naal Focus — experimental macOS prototype\n\nUsage: naal-focus [--settings | --check | --smoke-test]\n\nStarts in the menu bar, paused until explicitly enabled.\n--settings opens the settings window.\n--check reports desktop safety without showing a reminder.\n--smoke-test constructs native settings without showing windows or reminders.\n";

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("naal-focus {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{HELP}");
            return Ok(());
        }
        [flag] if flag == "--overlay" => return overlay::run(),
        [flag] if flag == "--check" => {
            let guard = safety::Guard::start(Settings::default().idle_secs);
            std::thread::sleep(std::time::Duration::from_millis(300));
            let reason = crate::reminder::blocked(
                guard.latest.lock().unwrap().as_ref(),
                crate::process::boot_time(),
            );
            println!(
                "Desktop safety: {}",
                reason
                    .as_deref()
                    .unwrap_or("ready (live validation still required)")
            );
            println!("No windows, reminders, or window history requested.");
            if reason.is_some() {
                return Err("Desktop safety is not ready.".into());
            }
            return Ok(());
        }
        [flag] if flag == "--smoke-test" => return preview(None),
        [flag, output] if flag == "--preview-settings" => return preview(Some(Path::new(output))),
        [] => {}
        [flag] if flag == "--settings" => {}
        _ => return Err("Unknown option; use --help.".into()),
    }
    let mtm = MainThreadMarker::new().ok_or("AppKit requires the main thread.")?;
    let path = config::config_path()?;
    let parent = path.parent().ok_or("Settings directory unavailable.")?;
    std::fs::create_dir_all(parent)?;
    let directory = private_fs::private_dir(parent)?;
    let lockfile = private_fs::open_at(&directory, c"app.lock", libc::O_RDWR | libc::O_CREAT)?;
    private_fs::check_owner(&lockfile)?;
    if !lockfile.metadata()?.is_file() {
        return Err("Invalid application lock.".into());
    }
    let _lock = private_fs::lock(lockfile)
        .map_err(|_| "Naal Focus is already running; use its menu-bar Settings item.")?;
    let settings = config::load(&path)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = Delegate::new(mtm, Some(Engine::start(settings, path)));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.finishLaunching();
    delegate.menu();
    observe_workspace(&delegate, sel!(desktopChanged:));
    *delegate.ivars().timer.borrow_mut() = Some(timer(&delegate, sel!(tick:), 0.25));
    if args == ["--settings"] {
        delegate.show_settings();
    }
    app.run();
    if let Some(timer) = delegate.ivars().timer.borrow_mut().take() {
        timer.invalidate();
    }
    // SAFETY: this retained delegate is the observer registered above.
    unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .removeObserver(&delegate);
    }
    Ok(())
}

fn observe_workspace(target: &AnyObject, selector: Sel) {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    for name in unsafe {
        [
            NSWorkspaceWillSleepNotification,
            NSWorkspaceDidWakeNotification,
            NSWorkspaceScreensDidSleepNotification,
            NSWorkspaceScreensDidWakeNotification,
            NSWorkspaceSessionDidResignActiveNotification,
            NSWorkspaceSessionDidBecomeActiveNotification,
        ]
    } {
        // SAFETY: both callers retain their main-thread target and implement the notification selector.
        unsafe {
            center.addObserver_selector_name_object(target, selector, Some(name), None);
        }
    }
}

// SAFETY: callers use a main-thread target with a matching one-argument timer selector.
fn timer(target: &AnyObject, selector: Sel, interval: f64) -> Retained<NSTimer> {
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
            interval, target, selector, None, true,
        );
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        timer
    }
}

#[derive(Default)]
struct State {
    engine: Option<Engine>,
    settings: RefCell<Option<SettingsWindow>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    status_label: OnceCell<Retained<NSMenuItem>>,
    enabled_item: OnceCell<Retained<NSMenuItem>>,
    snooze_item: OnceCell<Retained<NSMenuItem>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    quit_after_save: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. State is main-thread-only; no custom Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn keep_running(&self, _app: &NSApplication) -> bool { false }
    }
    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _window: &NSWindow) -> bool {
            self.confirm_close()
        }
    }
    impl Delegate {
        #[unsafe(method(desktopChanged:))]
        fn desktop_changed(&self, _notification: &NSNotification) {
            if let Some(engine) = &self.ivars().engine { engine.request(Request::Interrupt); }
        }
        #[unsafe(method(settings:))]
        fn settings_action(&self, _sender: &AnyObject) { self.show_settings(); }
        #[unsafe(method(enabled:))]
        fn enabled(&self, _sender: &AnyObject) {
            if let Some(engine) = &self.ivars().engine {
                let mut settings = engine.status().settings;
                settings.enabled = !settings.enabled;
                engine.request(Request::Settings(settings));
            }
        }
        #[unsafe(method(snooze:))]
        fn snooze(&self, _sender: &AnyObject) {
            if let Some(engine) = &self.ivars().engine {
                engine.request(if engine.status().snoozed { Request::Resume } else { Request::Snooze });
            }
        }
        #[unsafe(method(showNow:))]
        fn show_now(&self, _sender: &AnyObject) {
            if let Some(engine) = &self.ivars().engine { engine.request(Request::ShowNow); }
        }
        #[unsafe(method(page:))]
        fn page(&self, sender: &NSButton) {
            if let Some(form) = &*self.ivars().settings.borrow() { form.page(sender.tag() as usize); }
        }
        #[unsafe(method(save:))]
        fn save(&self, _sender: &AnyObject) { self.save_settings(false); }
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            if !self.confirm_close() {
                self.ivars().quit_after_save.set(self.ivars().settings.borrow().as_ref()
                    .is_some_and(|form| form.close_after_save && form.submitted.is_some()));
                return;
            }
            if let Some(engine) = &self.ivars().engine { engine.request(Request::Stop); }
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            let Some(engine) = &self.ivars().engine else { return; };
            let status = engine.status();
            if let Some(item) = self.ivars().status_label.get() {
                item.setTitle(&NSString::from_str(status.error.as_deref().unwrap_or(&status.message)));
            }
            if let Some(item) = self.ivars().enabled_item.get() {
                item.setState(if status.settings.enabled { NSControlStateValueOn } else { NSControlStateValueOff });
                item.setEnabled(!status.saving);
            }
            if let Some(item) = self.ivars().snooze_item.get() { item.setTitle(&NSString::from_str(if status.snoozed { "Resume" } else { "Snooze 15 min" })); }
            let mut quit = false;
            if let Some(form) = &mut *self.ivars().settings.borrow_mut() {
                form.save_button.setEnabled(!status.saving && form.submitted.is_none());
                if form.submitted.is_some() && !status.saving {
                    if let Some(error) = &status.error {
                        form.error.setStringValue(&NSString::from_str(error));
                        self.ivars().quit_after_save.set(false);
                    } else if form.close_after_save {
                        form.window.close();
                        quit = self.ivars().quit_after_save.replace(false);
                    }
                    form.submitted = None;
                    form.close_after_save = false;
                }
            }
            if quit {
                engine.request(Request::Stop);
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
        }
    }
);

impl Delegate {
    fn confirm_close(&self) -> bool {
        let Some(engine) = &self.ivars().engine else {
            return true;
        };
        let current = engine.status().settings;
        let dirty = self.ivars().settings.borrow().as_ref().is_some_and(|form| {
            form.window.isVisible() && form.read(&current).map_or(true, |draft| draft != current)
        });
        if !dirty {
            return true;
        }
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("Save changes?"));
        alert.addButtonWithTitle(&NSString::from_str("Save & close"));
        alert.addButtonWithTitle(&NSString::from_str("Discard"));
        alert.addButtonWithTitle(&NSString::from_str("Keep editing"));
        match alert.runModal() {
            1000 => {
                self.save_settings(true);
                false
            }
            1001 => true,
            _ => false,
        }
    }

    fn new(mtm: MainThreadMarker, engine: Option<Engine>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(State {
            engine,
            ..Default::default()
        });
        unsafe { msg_send![super(this), init] }
    }
    fn item(&self, menu: &NSMenu, title: &str, action: Option<Sel>) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &NSString::from_str(title),
                action,
                &NSString::from_str(""),
            )
        };
        unsafe {
            item.setTarget(Some(self));
        }
        item.setEnabled(action.is_some());
        menu.addItem(&item);
        item
    }
    fn menu(&self) {
        let mtm = self.mtm();
        let main = NSMenu::new(mtm);
        let application = NSMenu::new(mtm);
        self.item(&application, "Settings…", Some(sel!(settings:)))
            .setKeyEquivalent(&NSString::from_str(","));
        self.item(&application, "Quit Naal Focus", Some(sel!(quit:)))
            .setKeyEquivalent(&NSString::from_str("q"));
        let app_item = NSMenuItem::new(mtm);
        app_item.setTitle(&NSString::from_str("Naal Focus"));
        app_item.setSubmenu(Some(&application));
        main.addItem(&app_item);
        let edit = NSMenu::new(mtm);
        edit.setTitle(&NSString::from_str("Edit"));
        for (title, action, key) in [
            ("Undo", sel!(undo:), "z"),
            ("Redo", sel!(redo:), "Z"),
            ("Cut", sel!(cut:), "x"),
            ("Copy", sel!(copy:), "c"),
            ("Paste", sel!(paste:), "v"),
            ("Select All", sel!(selectAll:), "a"),
        ] {
            // SAFETY: standard AppKit editing selectors are resolved through the first-responder chain.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                )
            };
            edit.addItem(&item);
        }
        let edit_item = NSMenuItem::new(mtm);
        edit_item.setTitle(&NSString::from_str("Edit"));
        edit_item.setSubmenu(Some(&edit));
        main.addItem(&edit_item);
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&main));
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        self.ivars()
            .status_label
            .set(self.item(&menu, "Paused", None))
            .ok();
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.ivars()
            .enabled_item
            .set(self.item(&menu, "Recurring reminders", Some(sel!(enabled:))))
            .ok();
        self.item(&menu, "Show now", Some(sel!(showNow:)));
        self.ivars()
            .snooze_item
            .set(self.item(&menu, "Snooze 15 min", Some(sel!(snooze:))))
            .ok();
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.item(&menu, "Dashboard — Coming soon", None);
        self.item(&menu, "Settings…", Some(sel!(settings:)));
        self.item(&menu, "Quit Naal Focus", Some(sel!(quit:)));
        let status_item =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = status_item.button(mtm) {
            button.setTitle(&NSString::from_str("Naal"));
        }
        status_item.setMenu(Some(&menu));
        self.ivars().status_item.set(status_item).ok();
    }
    fn show_settings(&self) {
        let state = self.ivars();
        let current = state
            .engine
            .as_ref()
            .map(|engine| engine.status().settings)
            .unwrap_or_default();
        {
            let mut settings = state.settings.borrow_mut();
            if settings
                .as_ref()
                .is_none_or(|form| !form.window.isVisible())
            {
                *settings = Some(SettingsWindow::new(self.mtm(), self, &current));
            }
        }
        if let Some(form) = &*state.settings.borrow() {
            form.window.makeKeyAndOrderFront(None);
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
    }
    fn save_settings(&self, close: bool) {
        let Some(engine) = &self.ivars().engine else {
            return;
        };
        if engine.status().saving {
            return;
        }
        if let Some(form) = &mut *self.ivars().settings.borrow_mut() {
            match form.read(&engine.status().settings) {
                Ok(settings) => {
                    form.error.setStringValue(&NSString::from_str(""));
                    form.submitted = Some(settings.clone());
                    form.close_after_save = close;
                    engine.request(Request::Settings(settings));
                }
                Err(error) => form.error.setStringValue(&NSString::from_str(&error)),
            }
        }
    }
}

fn invalidate_tree(view: &NSView) {
    view.setNeedsDisplay(true);
    for child in view.subviews() {
        invalidate_tree(&child);
    }
}

fn preview(output: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let mtm = MainThreadMarker::new().ok_or("AppKit requires the main thread.")?;
    let _app = NSApplication::sharedApplication(mtm);
    let delegate = Delegate::new(mtm, None);
    let settings = Settings::default();
    let form = SettingsWindow::new(mtm, &delegate, &settings);
    for (width, height) in [(800.0, 600.0), (1000.0, 660.0), (1400.0, 900.0)] {
        form.window.setContentSize(NSSize::new(width, height));
        form.check_layout()?;
        assert_eq!(form.read(&settings)?, settings);
    }
    form.window.setContentSize(NSSize::new(1000.0, 660.0));
    if let Some(output) = output {
        std::fs::create_dir_all(output)?;
        for (name, index) in [
            ("reminders", 0),
            ("breathing", 1),
            ("reflections-coming-soon", 2),
            ("quotes", 6),
        ] {
            // Hidden windows can reuse incomplete subview caches between pages.
            let form = SettingsWindow::new(mtm, &delegate, &settings);
            form.window.setContentSize(NSSize::new(1000.0, 660.0));
            form.page(index);
            let view = form
                .window
                .contentView()
                .ok_or("Settings view unavailable.")?;
            invalidate_tree(&view);
            let bitmap = view
                .bitmapImageRepForCachingDisplayInRect(view.bounds())
                .ok_or("Settings capture unavailable.")?;
            view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &bitmap);
            if bitmap
                .colorAtX_y(10, 10)
                .is_none_or(|color| color.alphaComponent() < 0.99)
            {
                return Err("Settings capture has a missing background.".into());
            }
            // SAFETY: an empty properties dictionary is valid for PNG encoding.
            let data = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .ok_or("PNG encoding failed.")?;
            if !data.writeToFile_atomically(
                &NSString::from_str(
                    &output
                        .join(format!("settings-{name}.png"))
                        .to_string_lossy(),
                ),
                true,
            ) {
                return Err("Could not save settings capture.".into());
            }
        }
    }
    println!("Native settings smoke test passed; no reminders, focus capture, or personal data.");
    Ok(())
}
