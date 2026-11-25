//! Surfaces of the app's shown under a Wayland window (`NativeSurface::set_wayland_surface`).
//!
//! Each becomes a subsurface of the window's own `wl_surface`, placed below it and desynchronized,
//! so what the app commits to it reaches the screen as it commits it, without a frame of the
//! window's. The window punches the surface's place out of its pixels so the subsurface shows
//! through, and moves the subsurface with the place: a subsurface's position takes effect with the
//! parent's next commit, which is the frame that moved it. This binds `wl_subcompositor` on winit's
//! own display connection, in a private queue, as `activation` does.

use std::collections::HashSet;
use std::sync::Mutex;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use rune_media::native_surface::SurfaceDraw;
use rune_media::native_surface::linux::WaylandSurface;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use winit::window::Window;

/// The app surfaces some window has made its subsurface: a `wl_surface` may have one role, so a
/// surface moving to another window waits until the first lets it go.
static CLAIMED: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

fn claim(surface: usize) -> bool {
    CLAIMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashSet::new).insert(surface)
}

fn unclaim(surface: usize) {
    if let Some(claimed) = CLAIMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
        claimed.remove(&surface);
    }
}

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn event(_: &mut Self, _: &WlSubcompositor, _: <WlSubcompositor as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSubsurface, ()> for State {
    fn event(_: &mut Self, _: &WlSubsurface, _: <WlSubsurface as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// One app surface under the window.
struct Layer {
    /// The `NativeSurface` it shows (`NativeSurface::key`).
    key: usize,
    surface: WaylandSurface,
    subsurface: WlSubsurface,
    /// Where it is, in the window surface's logical coordinates.
    position: (i32, i32),
}

/// One Wayland window's app surfaces, made on first use and kept with the window.
pub(crate) struct SurfaceLayers {
    conn: Connection,
    queue: EventQueue<State>,
    subcompositor: WlSubcompositor,
    parent: WlSurface,
    layers: Vec<Layer>,
}

impl SurfaceLayers {
    /// None off Wayland, or when the compositor lacks `wl_subcompositor`.
    #[allow(unsafe_code)]
    pub(crate) fn new(window: &Window) -> Option<Self> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) =
            (window.display_handle().ok()?.as_raw(), window.window_handle().ok()?.as_raw())
        else {
            return None;
        };
        // SAFETY: winit owns this `wl_display` and keeps it alive for the event loop, which
        // outlives every window; a foreign backend never closes it.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let subcompositor = globals.bind::<WlSubcompositor, _, _>(&queue.handle(), 1..=1, ()).ok()?;
        // SAFETY: the pointer is the live `wl_surface` proxy winit created for this window, and
        // the layers are dropped before the window.
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) }.ok()?;
        let parent = WlSurface::from_id(&conn, id).ok()?;
        Some(Self { conn, queue, subcompositor, parent, layers: Vec::new() })
    }

    /// Puts the app surfaces among `surfaces` under the window at their places, given in physical
    /// pixels of a window at `scale`, and takes away those no longer drawn. Call it before the
    /// frame that draws them presents, which commits the positions. Returns the keys of the
    /// surfaces under the window, whose places the frame punches out.
    #[allow(unsafe_code)]
    pub(crate) fn sync(&mut self, surfaces: &[SurfaceDraw], scale: f64) -> Vec<usize> {
        let scale = scale.max(f64::EPSILON);
        let mut kept = Vec::with_capacity(surfaces.len());
        for draw in surfaces {
            let Some(surface) = draw.surface.wayland_surface() else { continue };
            let key = draw.surface.key();
            if kept.contains(&key) {
                continue;
            }
            let position = ((f64::from(draw.rect[0]) / scale).round() as i32, (f64::from(draw.rect[1]) / scale).round() as i32);
            let at = self.layers.iter().position(|layer| layer.key == key && layer.surface.same(&surface));
            let index = match at {
                Some(index) => index,
                None => {
                    // A surface still under another window (or under this one for another key)
                    // waits for that one to let it go.
                    if !claim(surface.as_ptr() as usize) {
                        continue;
                    }
                    // SAFETY: `WaylandSurface` promises a live `wl_surface` on this connection,
                    // kept alive by its handle, which the layer holds.
                    let child = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }
                        .ok()
                        .and_then(|id| WlSurface::from_id(&self.conn, id).ok());
                    let Some(child) = child else {
                        unclaim(surface.as_ptr() as usize);
                        continue;
                    };
                    let subsurface = self.subcompositor.get_subsurface(&child, &self.parent, &self.queue.handle(), ());
                    subsurface.place_below(&self.parent);
                    subsurface.set_desync();
                    subsurface.set_position(position.0, position.1);
                    self.layers.push(Layer { key, surface, subsurface, position });
                    self.layers.len() - 1
                }
            };
            let layer = &mut self.layers[index];
            if layer.position != position {
                layer.subsurface.set_position(position.0, position.1);
                layer.position = position;
            }
            kept.push(key);
        }
        self.layers.retain(|layer| {
            let keep = kept.contains(&layer.key);
            if !keep {
                remove(layer);
            }
            keep
        });
        self.queue.flush().ok();
        kept
    }

    /// Whether any app surface is under the window.
    pub(crate) fn any(&self) -> bool {
        !self.layers.is_empty()
    }
}

/// Unmaps `layer`'s surface: destroying the subsurface takes its role away, so another window may
/// take it.
fn remove(layer: &Layer) {
    layer.subsurface.destroy();
    unclaim(layer.surface.as_ptr() as usize);
}

impl Drop for SurfaceLayers {
    fn drop(&mut self) {
        for layer in &self.layers {
            remove(layer);
        }
        self.subcompositor.destroy();
        self.queue.flush().ok();
    }
}
