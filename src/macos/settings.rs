use super::Delegate;
use crate::{
    breathing::{Family, Pace},
    config::{self, Settings},
};
use objc2::{
    MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::*;
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

define_class!(
    // SAFETY: NSView has no additional subclassing requirements; drawing is main-thread-only.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct NaalSettingsBackground;
    unsafe impl NSObjectProtocol for NaalSettingsBackground {}
    impl NaalSettingsBackground {
        #[unsafe(method(isOpaque))]
        fn opaque(&self) -> bool { true }
        #[unsafe(method(drawRect:))]
        fn draw(&self, _rect: NSRect) {
            // Hidden-view captures do not include NSWindow’s separate background.
            NSColor::windowBackgroundColor().setFill();
            NSRectFill(self.bounds());
        }
    }
);

pub(super) struct SettingsWindow {
    pub window: Retained<NSWindow>,
    pages: Vec<Retained<NSView>>,
    interval_minutes: Retained<NSTextField>,
    interval_seconds: Retained<NSTextField>,
    duration: Retained<NSTextField>,
    idle_minutes: Retained<NSTextField>,
    idle_seconds: Retained<NSTextField>,
    breathing: Retained<NSButton>,
    reduced_motion: Retained<NSButton>,
    pace: Retained<NSPopUpButton>,
    cycles: Retained<NSPopUpButton>,
    families: Vec<Retained<NSButton>>,
    text_size: Retained<NSTextField>,
    quotes: Retained<NSTextView>,
    pub error: Retained<NSTextField>,
    pub save_button: Retained<NSButton>,
    pub submitted: Option<Settings>,
    pub close_after_save: bool,
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}
fn place(view: &NSView, parent: &NSView, top: f64, x: f64, width: f64, height: f64) {
    view.setFrame(rect(
        x,
        parent.bounds().size.height - top - height,
        width,
        height,
    ));
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    parent.addSubview(view);
}
fn label(mtm: MainThreadMarker, parent: &NSView, title: &str, top: f64) {
    let field = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    place(&field, parent, top, 22.0, 270.0, 24.0);
}
fn number(
    mtm: MainThreadMarker,
    parent: &NSView,
    value: u64,
    top: f64,
    x: f64,
) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(&value.to_string()), mtm);
    place(&field, parent, top, x, 70.0, 26.0);
    field
}
fn checkbox(
    mtm: MainThreadMarker,
    parent: &NSView,
    title: &str,
    checked: bool,
    top: f64,
) -> Retained<NSButton> {
    // No target: AppKit handles the native checkbox state.
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    place(&button, parent, top, 22.0, 440.0, 26.0);
    button.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    button
}
fn popup(
    mtm: MainThreadMarker,
    parent: &NSView,
    items: &[&str],
    selected: usize,
    top: f64,
) -> Retained<NSPopUpButton> {
    let button = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(0.0, 0.0, 240.0, 28.0),
        false,
    );
    for title in items {
        button.addItemWithTitle(&NSString::from_str(title));
    }
    button.selectItemAtIndex(selected as isize);
    place(&button, parent, top, 290.0, 230.0, 28.0);
    button
}

impl SettingsWindow {
    pub fn new(mtm: MainThreadMarker, target: &Delegate, settings: &Settings) -> Self {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 900.0, 660.0),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setContentMinSize(NSSize::new(800.0, 600.0));
        }
        window.setTitle(&NSString::from_str("Naal Focus Settings"));
        window.setDelegate(Some(ProtocolObject::from_ref(target)));
        let allocated = NaalSettingsBackground::alloc(mtm).set_ivars(());
        // SAFETY: this initializes the subclass’s NSView superclass with a valid frame.
        let root: Retained<NaalSettingsBackground> =
            unsafe { msg_send![super(allocated), initWithFrame: rect(0.0, 0.0, 900.0, 660.0)] };
        window.setContentView(Some(&root));
        let sidebar = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 64.0, 200.0, 596.0));
        sidebar.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
        root.addSubview(&sidebar);
        let mut pages = Vec::new();
        for (index, title) in [
            "Reminders",
            "Breathing",
            "Reflections",
            "Reflection charts",
            "Window history",
            "Appearance",
            "Quotes",
        ]
        .into_iter()
        .enumerate()
        {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(title),
                    Some(target),
                    Some(sel!(page:)),
                    mtm,
                )
            };
            button.setTag(index as isize);
            place(
                &button,
                &sidebar,
                20.0 + index as f64 * 44.0,
                12.0,
                176.0,
                32.0,
            );
            let page = NSView::initWithFrame(NSView::alloc(mtm), rect(210.0, 64.0, 680.0, 596.0));
            page.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            page.setHidden(index != 0);
            label(mtm, &page, title, 24.0);
            if [2, 3, 4].contains(&index) {
                label(mtm, &page, "Coming soon", 76.0);
            }
            root.addSubview(&page);
            pages.push(page);
        }
        let reminders = &pages[0];
        label(
            mtm,
            reminders,
            "Reminder interval · minutes / seconds",
            90.0,
        );
        let interval_minutes = number(mtm, reminders, settings.interval_secs / 60, 86.0, 360.0);
        let interval_seconds = number(mtm, reminders, settings.interval_secs % 60, 86.0, 446.0);
        label(mtm, reminders, "Quote duration · seconds", 146.0);
        let duration = number(mtm, reminders, settings.duration_secs, 142.0, 360.0);
        label(mtm, reminders, "Idle threshold · minutes / seconds", 202.0);
        let idle_minutes = number(
            mtm,
            reminders,
            u64::from(settings.idle_secs) / 60,
            198.0,
            360.0,
        );
        let idle_seconds = number(
            mtm,
            reminders,
            u64::from(settings.idle_secs) % 60,
            198.0,
            446.0,
        );
        let page = &pages[1];
        let breathing = checkbox(
            mtm,
            page,
            "Breathing entrance",
            settings.breathing.enabled,
            72.0,
        );
        let reduced_motion = checkbox(
            mtm,
            page,
            "Reduced motion",
            settings.breathing.reduced_motion,
            112.0,
        );
        label(mtm, page, "Pace", 158.0);
        let selected = Pace::ALL
            .iter()
            .position(|pace| *pace == settings.breathing.pace)
            .unwrap_or(1);
        let pace = popup(
            mtm,
            page,
            &[
                "Short · 3s in / 4s out",
                "Balanced · 4s in / 6s out",
                "Even · 5s in / 5s out",
            ],
            selected,
            154.0,
        );
        label(mtm, page, "Breaths", 202.0);
        let cycles = popup(
            mtm,
            page,
            &["One", "Two"],
            usize::from(settings.breathing.cycles.saturating_sub(1)),
            198.0,
        );
        let families = Family::ALL
            .iter()
            .enumerate()
            .map(|(index, family)| {
                checkbox(
                    mtm,
                    page,
                    family.label(),
                    settings.breathing.families.contains(family),
                    254.0 + index as f64 * 38.0,
                )
            })
            .collect();
        label(mtm, &pages[5], "Quote text size", 90.0);
        let text_size = number(mtm, &pages[5], u64::from(settings.text_size), 86.0, 320.0);
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(22.0, 24.0, 636.0, 480.0));
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scroll.setHasVerticalScroller(true);
        let quotes =
            NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 616.0, 480.0));
        quotes.setRichText(false);
        quotes.setVerticallyResizable(true);
        quotes.setHorizontallyResizable(false);
        quotes.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        if let Some(container) = unsafe { quotes.textContainer() } {
            container.setWidthTracksTextView(true);
        }
        quotes.setFont(Some(&NSFont::systemFontOfSize(14.0)));
        quotes.setString(&NSString::from_str(&config::format_quotes(
            &settings.quotes,
        )));
        scroll.setDocumentView(Some(&quotes));
        pages[6].addSubview(&scroll);
        let save_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Save"),
                Some(target),
                Some(sel!(save:)),
                mtm,
            )
        };
        save_button.setFrame(rect(790.0, 18.0, 90.0, 32.0));
        save_button.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        root.addSubview(&save_button);
        let error = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        error.setFrame(rect(222.0, 10.0, 550.0, 46.0));
        error.setTextColor(Some(&NSColor::systemRedColor()));
        error.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        root.addSubview(&error);
        window.center();
        Self {
            window,
            pages,
            interval_minutes,
            interval_seconds,
            duration,
            idle_minutes,
            idle_seconds,
            breathing,
            reduced_motion,
            pace,
            cycles,
            families,
            text_size,
            quotes,
            error,
            save_button,
            submitted: None,
            close_after_save: false,
        }
    }

    pub fn page(&self, index: usize) {
        for (page_index, page) in self.pages.iter().enumerate() {
            page.setHidden(page_index != index);
        }
    }
    pub fn check_layout(&self) -> Result<(), String> {
        for page in &self.pages {
            let bounds = page.bounds();
            for view in page.subviews() {
                let frame = view.frame();
                if frame.origin.x < 0.0
                    || frame.origin.y < 0.0
                    || frame.origin.x + frame.size.width > bounds.size.width + 1.0
                    || frame.origin.y + frame.size.height > bounds.size.height + 1.0
                {
                    return Err("A settings control exceeds its page bounds.".into());
                }
            }
        }
        Ok(())
    }

    pub fn read(&self, current: &Settings) -> Result<Settings, String> {
        let read = |field: &NSTextField| {
            field
                .stringValue()
                .to_string()
                .trim()
                .parse::<u64>()
                .map_err(|_| "Enter whole, nonnegative numbers.".to_string())
        };
        let pair = |minutes: &NSTextField, seconds: &NSTextField| -> Result<u64, String> {
            let seconds = read(seconds)?;
            if seconds >= 60 {
                return Err("Seconds must be between 0 and 59.".into());
            }
            read(minutes)?
                .checked_mul(60)
                .and_then(|m| m.checked_add(seconds))
                .ok_or("Time value is too large.".into())
        };
        let mut settings = current.clone();
        settings.interval_secs = pair(&self.interval_minutes, &self.interval_seconds)?;
        settings.duration_secs = read(&self.duration)?;
        settings.idle_secs = pair(&self.idle_minutes, &self.idle_seconds)?
            .try_into()
            .map_err(|_| "Idle threshold is too large.")?;
        settings.text_size = read(&self.text_size)?
            .try_into()
            .map_err(|_| "Text size is too large.")?;
        settings.breathing.enabled = self.breathing.state() == NSControlStateValueOn;
        settings.breathing.reduced_motion = self.reduced_motion.state() == NSControlStateValueOn;
        settings.breathing.pace = *Pace::ALL
            .get(self.pace.indexOfSelectedItem() as usize)
            .ok_or("Choose a breathing pace.")?;
        settings.breathing.cycles = (self.cycles.indexOfSelectedItem() + 1)
            .try_into()
            .map_err(|_| "Choose one or two breaths.")?;
        settings.breathing.families = Family::ALL
            .into_iter()
            .zip(&self.families)
            .filter_map(|(family, button)| {
                (button.state() == NSControlStateValueOn).then_some(family)
            })
            .collect();
        settings.quotes = config::parse_quotes(&self.quotes.string().to_string())?;
        settings.validate()?;
        Ok(settings)
    }
}
