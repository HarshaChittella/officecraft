//! Runs each Craft engine in an egui [`Context`](egui::Context) of its own (docs/decisions.md
//! D1) and paints it with egui-wgpu into a texture.
//!
//! [`EngineHost`] does for one engine what eframe does for one app: it carries input over
//! between passes, calls the input hook, runs `logic` and `ui` in one pass (or `logic` alone
//! while the engine isn't shown, D4), applies texture changes, paints, and turns the frame's
//! output into a [`FrameResult`] the window can act on. Window commands the apps send are
//! handled by the D18 policy in [`viewport_policy`].
//!
//! Phase 1 paints into a texture (the test harness shows it inside its own window); Phase 2
//! adds a surface on a `CAMetalLayer`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod embed;
pub mod viewport_policy;

use craftworks_core::Editor;
use std::time::{Duration, Instant};

/// The clock. The workspace bans `Instant::now` because it traps on wasm (`clippy.toml`); the
/// render host runs only natively.
#[allow(clippy::disallowed_methods)]
fn now() -> Instant {
    Instant::now()
}

/// What one frame produced that the window has to act on.
#[derive(Default)]
pub struct FrameResult {
    /// Cursor, copied text, opened URLs, IME rectangle, accessibility update…
    pub platform: egui::PlatformOutput,
    /// When the engine wants its next frame (zero: as soon as possible; `Duration::MAX`: only
    /// on input).
    pub repaint_after: Duration,
    /// The app asked to close the window (`ViewportCommand::Close`).
    pub close: bool,
    /// The app refused a close the window asked for (`ViewportCommand::CancelClose`).
    pub cancel_close: bool,
    /// The whole font atlas was uploaded this frame (fonts installed or rebuilt).
    pub font_atlas_rebuilt: bool,
    /// Time spent in `logic` + `ui` + tessellation + recording the paint.
    pub cpu: Duration,
}

/// The render target an engine paints into.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// One engine, its egui context and its renderer.
pub struct EngineHost {
    ctx: egui::Context,
    editor: Box<dyn Editor>,
    renderer: egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
    target: Option<Target>,
    /// Input that arrived while only `logic` ran: the next full pass gets it.
    pending: egui::RawInput,
    start: Instant,
    last_logic: Option<Instant>,
    font_rebuilds: u32,
    font_atlas: Option<[usize; 2]>,
}

impl EngineHost {
    /// A host for `editor` that paints into textures of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, editor: Box<dyn Editor>) -> Self {
        let ctx = egui::Context::default();
        // Extra windows (View › New Window, presenter view) draw as windows inside the tab
        // until the shell gives them windows of their own (D13).
        ctx.set_embed_viewports(true);
        EngineHost {
            ctx,
            editor,
            renderer: egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default()),
            format,
            target: None,
            pending: egui::RawInput::default(),
            start: now(),
            last_logic: None,
            font_rebuilds: 0,
            font_atlas: None,
        }
    }

    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn editor(&self) -> &dyn Editor {
        self.editor.as_ref()
    }

    pub fn editor_mut(&mut self) -> &mut dyn Editor {
        self.editor.as_mut()
    }

    /// The texture the last frame was painted into.
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|t| &t.texture)
    }

    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.view)
    }

    /// How many frames uploaded the whole font atlas (fonts installed or rebuilt), and its size
    /// in pixels the last time.
    pub fn font_stats(&self) -> (u32, Option<[usize; 2]>) {
        (self.font_rebuilds, self.font_atlas)
    }

    /// Whether the background tick (D4) is due: `logic` hasn't run for `every`.
    pub fn logic_due(&self, every: Duration) -> bool {
        self.last_logic.is_none_or(|t| t.elapsed() >= every)
    }

    /// One shown frame: `input` (in this engine's own points, origin at its top left) goes to
    /// the app, `logic` and `ui` run in one pass, and the result is painted into a texture of
    /// `size_px` physical pixels. Returns what the window has to act on.
    pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: egui::RawInput, size_px: [u32; 2]) -> FrameResult {
        let t0 = now();
        let mut raw = std::mem::take(&mut self.pending);
        raw.append(input);
        raw.time = Some(self.start.elapsed().as_secs_f64());
        let editor = &mut self.editor;
        editor.raw_input_hook(&self.ctx, &mut raw);
        let mut out = self.ctx.run_ui(raw, |ui| {
            editor.logic(ui.ctx());
            editor.ui(ui);
        });
        self.last_logic = Some(now());

        let mut result = FrameResult::default();
        // The font atlas is the managed texture 0; a whole upload means fonts were (re)built.
        if let Some(whole) = out.textures_delta.set.get(&egui::TextureId::default()).and_then(|deltas| deltas.iter().rev().find(|d| d.is_whole())) {
            result.font_atlas_rebuilt = true;
            self.font_rebuilds = self.font_rebuilds.saturating_add(1);
            self.font_atlas = Some(whole.image.size());
        }
        #[expect(clippy::iter_over_hash_type)] // order doesn't matter: each texture is independent
        for (id, deltas) in out.textures_delta.set.drain() {
            for delta in deltas {
                self.renderer.update_texture(device, queue, id, &delta);
            }
        }
        let jobs = self.ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size_px, pixels_per_point: out.pixels_per_point };
        self.paint(device, queue, &jobs, &screen);
        #[expect(clippy::iter_over_hash_type)]
        for id in out.textures_delta.free.drain() {
            self.renderer.free_texture(&id);
        }

        let mut viewport_outputs = std::mem::take(&mut out.viewport_output);
        if let Some(root) = viewport_outputs.remove(&egui::ViewportId::ROOT) {
            result.repaint_after = root.repaint_delay;
            let handled = viewport_policy::apply(self.editor.kind(), egui::ViewportId::ROOT, root.commands);
            result.close = handled.close;
            result.cancel_close = handled.cancel_close;
        }
        // Embedded viewports draw inside the root pass; any other output is still worth a note.
        for (id, vp) in viewport_outputs {
            viewport_policy::apply(self.editor.kind(), id, vp.commands);
        }
        result.platform = std::mem::take(&mut out.platform_output);
        result.cpu = t0.elapsed();
        result
    }

    /// A frame for an engine that isn't shown: only `logic` runs (`Context::run_logic`), so
    /// autosave, background jobs and requests keep going. `input` carries the window state; its
    /// events wait for the next shown frame.
    pub fn logic_only(&mut self, input: egui::RawInput) -> FrameResult {
        let t0 = now();
        let mut raw = std::mem::take(&mut self.pending);
        raw.append(input);
        raw.time = Some(self.start.elapsed().as_secs_f64());
        let editor = &mut self.editor;
        editor.raw_input_hook(&self.ctx, &mut raw);
        let out = self.ctx.run_logic(&raw, |ctx| editor.logic(ctx));
        self.pending = raw;
        self.last_logic = Some(now());
        let mut result = FrameResult::default();
        for (id, commands) in out.viewport_commands {
            let handled = viewport_policy::apply(self.editor.kind(), id, commands);
            if id == egui::ViewportId::ROOT {
                result.close = handled.close;
                result.cancel_close = handled.cancel_close;
            }
        }
        result.platform = out.platform_output;
        result.repaint_after = Duration::MAX;
        result.cpu = t0.elapsed();
        result
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("craftworks engine"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.target = Some(Target { texture, view, size });
    }

    fn paint(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, jobs: &[egui::ClippedPrimitive], screen: &egui_wgpu::ScreenDescriptor) {
        self.ensure_target(device, screen.size_in_pixels);
        let Some(target) = &self.target else { return };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("craftworks engine") });
        let mut buffers = self.renderer.update_buffers(device, queue, &mut encoder, jobs, screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftworks engine"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), jobs, screen);
        }
        buffers.push(encoder.finish());
        queue.submit(buffers);
    }
}
