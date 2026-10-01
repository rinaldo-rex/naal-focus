//! One coordinated, short-lived native overlay on every selected output.
use crate::{
    config::{Settings, parse_quotes},
    guard::Guard,
    process::{Singleton, boot_time, desktop_connection},
    render,
};
use fontdue::Font;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::{collections::HashMap, error::Error, io::Read, time::Duration};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

pub const HELP: &str = "Naal Focus — one-shot overlay helper for COSMIC/Wayland

Usage: naal-focus-overlay [OPTIONS]
  --check              Read-only protocol, monitor, and session safety probe
  --once               Show one reminder (default); never runs its own timer
  --all                Cover all monitors (default)
  --output NAME        Cover one named monitor for manual testing
  --duration-secs N    Visible timeout, 1–30 (default 5)
  --delay-secs N       Delay before showing, 0–60 (default 0)
  --text-size N        Logical text size, 22–72 (default 36)
  --idle-secs N        Skip idle sessions, 30–3600 (default 120)
  --stdin              Read one bounded UTF-8 quote from stdin
  -h, --help           Print help

WARNING: captures keyboard focus on all covered monitors. Escape or a mouse
button release dismisses ALL surfaces. Other keys are consumed, never logged.
Always auto-dismisses. This is not a screen lock or security boundary.
";

#[derive(Debug, Default)]
pub struct Options {
    pub settings: Settings,
    pub check: bool,
    pub delay: u64,
    pub output: Option<String>,
    pub stdin: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self::default();
        let mut all = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--check" => options.check = true,
                "--once" => {}
                "--all" => all = true,
                "--stdin" => options.stdin = true,
                "--output" => {
                    options.output = Some(
                        args.next()
                            .filter(|v| !v.is_empty() && !v.starts_with('-'))
                            .ok_or("--output requires a monitor name.")?,
                    )
                }
                "--duration-secs" => options.settings.duration_secs = number(&arg, args.next())?,
                "--delay-secs" => options.delay = number(&arg, args.next())?,
                "--text-size" => {
                    options.settings.text_size = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Text size is too large.")?
                }
                "--idle-secs" => {
                    options.settings.idle_secs = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Idle threshold is too large.")?
                }
                _ => return Err(format!("Unknown option {arg}; use --help.")),
            }
        }
        options.settings.validate()?;
        if options.delay > 60 {
            return Err("Delay must be 0–60 seconds.".into());
        }
        if all && options.output.is_some() {
            return Err("Choose --all or --output, not both.".into());
        }
        Ok(Some(options))
    }
}
fn number(option: &str, text: Option<String>) -> Result<u64, String> {
    text.ok_or_else(|| format!("{option} requires an integer."))?
        .parse()
        .map_err(|_| format!("{option} requires an integer."))
}

pub fn run(mut options: Options) -> Result<(), Box<dyn Error>> {
    if options.stdin && !options.check {
        let mut quote = String::new();
        std::io::stdin().take(4097).read_to_string(&mut quote)?;
        if quote.len() > 4096 {
            return Err("Quote input is too large.".into());
        }
        let quotes = parse_quotes(&quote)?;
        if quotes.len() != 1 {
            return Err("Helper requires exactly one quote.".into());
        }
        options.settings.quotes = quotes;
    }
    let conn = desktop_connection()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("Desktop does not expose wlr-layer-shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        pool: SlotPool::new(4096, &shm)?,
        shm,
        keyboards: HashMap::new(),
        pointers: HashMap::new(),
        active: Vec::new(),
        reason: None,
        failure: None,
        font: None,
        quote: options.settings.quotes[0].clone(),
        text_size: options.settings.text_size,
        requested_at: None,
        mapped_at: None,
    };
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    let mut selected = Vec::new();
    for output in app.outputs.outputs() {
        if let Some(info) = app.outputs.info(&output) {
            let name = info.name.as_deref().unwrap_or("(unnamed)");
            println!(
                "Output: {name} · logical {:?} · scale {}",
                info.logical_size, info.scale_factor
            );
            if options.output.as_ref().is_none_or(|wanted| wanted == name) {
                selected.push(output);
            }
        }
    }
    if selected.is_empty() {
        return Err("No matching outputs; use --check to list monitors.".into());
    }
    let guard = Guard::start(options.settings.idle_secs);
    let wait_started = boot_time();
    loop {
        let status = guard.snapshot();
        if status.session_seen.is_some() && status.idle_seen.is_some() {
            let blocked = status.blocked_reason(boot_time());
            println!(
                "Session guard: {}",
                blocked
                    .as_deref()
                    .unwrap_or("ready (active, unlocked, not idle)")
            );
            if options.check {
                println!(
                    "Desktop layer-shell and shared-memory rendering available; no surfaces created."
                );
                return Ok(());
            }
            if let Some(reason) = blocked {
                return Err(reason.into());
            }
            break;
        }
        if boot_time() - wait_started > Duration::from_secs(3) {
            return Err(status
                .blocked_reason(boot_time())
                .unwrap_or_else(|| "Guard startup timed out.".into())
                .into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _singleton = Singleton::acquire("overlay")?;
    app.font = Some(render::default_font()?);
    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle())?;
    let show_at = boot_time() + Duration::from_secs(options.delay);
    loop {
        let now = boot_time();
        if let Some(reason) = guard.snapshot().blocked_reason(now) {
            app.active.clear();
            conn.flush()?;
            println!("Reminder skipped/cancelled: {reason}");
            return Ok(());
        }
        if app.requested_at.is_none() && now >= show_at {
            app.show(&qh, &selected);
            arm_watchdog(options.settings.duration_secs + 8);
        }
        event_loop.dispatch(Duration::from_millis(50), &mut app)?;
        if let Some(mapped_at) = app.mapped_at
            && boot_time().saturating_sub(mapped_at)
                >= Duration::from_secs(options.settings.duration_secs)
        {
            app.reason = Some("timeout");
        }
        if app
            .requested_at
            .is_some_and(|at| boot_time().saturating_sub(at) >= Duration::from_secs(3))
            && app.active.iter().any(|a| !a.mapped)
        {
            app.failure = Some("Not every monitor configured within 3 seconds.".into());
        }
        if let Some(error) = app.failure.take() {
            app.active.clear();
            conn.flush()?;
            return Err(error.into());
        }
        if let Some(reason) = app.reason.take() {
            println!(
                "Dismissed ALL outputs: {reason} · mapped={} · keyboard-focus-observed={}",
                app.active.iter().filter(|a| a.mapped).count(),
                app.active.iter().any(|a| a.keyboard_focus_observed)
            );
            app.active.clear();
            conn.flush()?;
            return Ok(());
        }
    }
}

fn arm_watchdog(seconds: u64) {
    let deadline = boot_time() + Duration::from_secs(seconds);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            if boot_time() >= deadline {
                eprintln!("Overlay hard deadline exceeded; terminating to release focus.");
                std::process::exit(1);
            }
        }
    });
}

struct Active {
    layer: LayerSurface,
    output: wl_output::WlOutput,
    size: (u32, u32),
    scale: i32,
    mapped: bool,
    keyboard_focus_observed: bool,
}
struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    keyboards: HashMap<wl_seat::WlSeat, wl_keyboard::WlKeyboard>,
    pointers: HashMap<wl_seat::WlSeat, wl_pointer::WlPointer>,
    active: Vec<Active>,
    reason: Option<&'static str>,
    failure: Option<String>,
    font: Option<Font>,
    quote: String,
    text_size: u32,
    requested_at: Option<Duration>,
    mapped_at: Option<Duration>,
}
impl App {
    fn show(&mut self, qh: &QueueHandle<Self>, outputs: &[wl_output::WlOutput]) {
        self.requested_at = Some(boot_time());
        for output in outputs {
            let layer = self.layer_shell.create_layer_surface(
                qh,
                self.compositor.create_surface(qh),
                Layer::Overlay,
                Some("naal-focus-overlay"),
                Some(output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.commit();
            let scale = self
                .outputs
                .info(output)
                .map_or(1, |info| info.scale_factor.max(1));
            self.active.push(Active {
                layer,
                output: output.clone(),
                size: (0, 0),
                scale,
                mapped: false,
                keyboard_focus_observed: false,
            });
        }
        println!(
            "Requested {} fullscreen overlays · exclusive keyboard · coordinated dismissal",
            outputs.len()
        );
    }
    fn draw(&mut self, index: usize) -> Result<(), Box<dyn Error>> {
        let active = &self.active[index];
        let scale = active.scale.clamp(1, 4) as u32;
        let width = active.size.0.checked_mul(scale).ok_or("Width overflow")?;
        let height = active.size.1.checked_mul(scale).ok_or("Height overflow")?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32_000_000 {
            return Err(format!("Unsupported size {width}x{height}").into());
        }
        let total: u64 = self
            .active
            .iter()
            .map(|a| {
                u64::from(a.size.0) * u64::from(a.size.1) * (a.scale.clamp(1, 4) as u64).pow(2)
            })
            .sum();
        if total > 64_000_000 {
            return Err("Combined monitor buffers exceed the safety budget.".into());
        }
        let (buffer, canvas) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        render::render(
            canvas,
            width as usize,
            height as usize,
            self.font.as_ref().ok_or("Font not loaded")?,
            &self.quote,
            self.text_size as f32 * scale as f32,
            scale as f32,
        );
        let active = &mut self.active[index];
        let surface = active.layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        buffer.attach_to(surface)?;
        active.layer.commit();
        if !active.mapped {
            active.mapped = true;
            self.mapped_at.get_or_insert_with(boot_time);
            let name = self
                .outputs
                .info(&active.output)
                .and_then(|i| i.name)
                .unwrap_or_else(|| "(unnamed)".into());
            println!("Buffer committed: {name} · {width}x{height} · scale {scale}");
        }
        Ok(())
    }
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.active.iter().any(|a| &a.layer == layer) {
            self.reason = Some("compositor-closed");
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(index) = self.active.iter().position(|a| &a.layer == layer) {
            self.active[index].size = config.new_size;
            if let Err(e) = self.draw(index) {
                self.failure = Some(e.to_string());
            }
        }
    }
}
impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        scale: i32,
    ) {
        if let Some(index) = self
            .active
            .iter()
            .position(|a| a.layer.wl_surface() == surface)
        {
            self.active[index].scale = scale;
            if self.active[index].mapped
                && let Err(e) = self.draw(index)
            {
                self.failure = Some(e.to_string());
            }
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if self.requested_at.is_some() {
            self.reason = Some("output-added");
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.reason = Some("output-removed");
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if let Some(k) = self.keyboards.remove(&seat) {
            k.release();
        }
        if let Some(p) = self.pointers.remove(&seat) {
            p.release();
        }
        if self.requested_at.is_some() {
            self.reason = Some("seat-removed");
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if !self.keyboards.contains_key(&seat) => {
                match self.seats.get_keyboard(qh, &seat, None) {
                    Ok(k) => {
                        self.keyboards.insert(seat, k);
                    }
                    Err(e) => self.failure = Some(format!("Keyboard setup: {e}")),
                }
            }
            Capability::Pointer if !self.pointers.contains_key(&seat) => {
                match self.seats.get_pointer(qh, &seat) {
                    Ok(p) => {
                        self.pointers.insert(seat, p);
                    }
                    Err(e) => self.failure = Some(format!("Pointer setup: {e}")),
                }
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboards.remove(&seat)
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointers.remove(&seat)
        {
            p.release();
        }
    }
}
impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if let Some(active) = self
            .active
            .iter_mut()
            .find(|a| a.layer.wl_surface() == surface)
        {
            active.keyboard_focus_observed = true;
            println!("Keyboard focus acquired by overlay");
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if !self.active.is_empty() && event.keysym == Keysym::Escape {
            self.reason = Some("escape");
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}
impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if self
                .active
                .iter()
                .any(|a| a.layer.wl_surface() == &event.surface)
                && matches!(event.kind, PointerEventKind::Release { .. })
            {
                self.reason = Some("click");
            }
        }
    }
}
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
delegate_registry!(App);
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(App);

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(args.iter().map(|s| (*s).into()))
    }
    #[test]
    fn helper_has_no_recurring_mode_and_bounds_focus_capture() {
        assert!(parse(&[]).unwrap().unwrap().output.is_none());
        for args in [
            vec!["--loop"],
            vec!["--duration-secs", "0"],
            vec!["--duration-secs", "31"],
            vec!["--delay-secs", "61"],
            vec!["--all", "--output", "DP-1"],
            vec!["--output"],
            vec!["--text-size", "21"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert!(parse(&["--check", "--output", "DP-1"]).is_ok());
    }
}
