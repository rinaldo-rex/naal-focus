use super::{Active, App, interaction::pointer_input};
use crate::{process::boot_time, recall::PendingHistory, render};
use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

impl App {
    pub(super) fn show(&mut self, qh: &QueueHandle<Self>, outputs: &[wl_output::WlOutput]) {
        self.requested_at = Some(boot_time());
        if self.reflections_enabled {
            match PendingHistory::start() {
                Ok(pending) => self.pending_history = Some(pending),
                Err(error) => {
                    self.history_unavailable = true;
                    eprintln!("Previous reflections unavailable: {error}");
                }
            }
        }
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
                buffers: Vec::new(),
                chart_versions: Vec::new(),
                entrance_dirty: Vec::new(),
                entrance_backdrops: Vec::new(),
                reveal_cache: None,
                buffer_size: (0, 0, 0),
            });
        }
        println!(
            "Requested {} fullscreen overlays · exclusive keyboard · coordinated dismissal",
            outputs.len()
        );
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
            if let Err(e) = self.draw(index, boot_time()) {
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
                && let Err(e) = self.draw(index, boot_time())
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
                match self.seats.get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.loop_handle.clone(),
                    Box::new(|app, keyboard, event| app.key(keyboard, event, true)),
                ) {
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
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(keyboard, event, false);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(keyboard, event, true);
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
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers.insert(keyboard.clone(), modifiers);
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
        if !self.reflection_input_ready(boot_time()) {
            return;
        }
        for event in events {
            let Some(active) = self
                .active
                .iter()
                .find(|a| a.layer.wl_surface() == &event.surface)
            else {
                continue;
            };
            let scale = active.scale.clamp(1, 4) as f32;
            let content = render::focus_content(
                active.size.0 as f32 * scale,
                active.size.1 as f32 * scale,
                scale,
                &self.focus_settings,
            );
            self.dirty |= pointer_input(
                &mut self.activity,
                self.font.as_ref(),
                &event.kind,
                ((content.width / scale).round() as u32, active.size.1),
                scale,
                (
                    event.position.0 - f64::from(content.x / scale),
                    event.position.1,
                ),
                boot_time(),
            );
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
