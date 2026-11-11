use crate::model::{content_rect, Scene, HEIGHT, WIDTH};
use crate::xp3::{Assets, Store};
use foreign_types::ForeignTypeRef;
use metal::*;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

const SHADER: &str = include_str!("scene.metal");
const CACHE_LIMIT: usize = 160 * 1024 * 1024;
const TEXTURE_LIMIT: usize = 256 * 1024 * 1024;
struct Cached {
    texture: Texture,
    bytes: usize,
    used: u64,
}
pub struct Renderer {
    device: Device,
    queue: CommandQueue,
    pipeline: RenderPipelineState,
    cache: HashMap<String, Cached>,
    clock: u64,
}

impl Renderer {
    /// # Safety
    /// Supply a live CAMetalLayer with a non-null Metal device on the main thread.
    pub unsafe fn new(layer: *mut c_void) -> Result<Self, String> {
        if layer.is_null() {
            return Err("null CAMetalLayer".into());
        }
        let layer = MetalLayerRef::from_ptr(layer.cast());
        let device = layer.device().to_owned();
        let queue = device.new_command_queue();
        let library = device.new_library_with_source(SHADER, &CompileOptions::new())?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library.get_function("quad_vertex", None)?;
        let fragment = library.get_function("quad_fragment", None)?;
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        let attachment = descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("no Metal color attachment")?;
        attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        attachment.set_blending_enabled(true);
        attachment.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
        attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
        attachment.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        let pipeline = device.new_render_pipeline_state(&descriptor)?;
        Ok(Self {
            device,
            queue,
            pipeline,
            cache: HashMap::new(),
            clock: 0,
        })
    }
    fn texture(&mut self, store: &Store, name: &str, active: &HashSet<&str>) -> Result<(), String> {
        if let Some(texture) = self.cache.get_mut(name) {
            texture.used = self.clock;
            return Ok(());
        }
        let bytes = store.bytes(name).map_err(|e| e.to_string())?;
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let rgba = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        let mut total: usize = self.cache.values().map(|v| v.bytes).sum();
        while total + rgba.len() > TEXTURE_LIMIT {
            let oldest = self
                .cache
                .iter()
                .filter(|(key, _)| !active.contains(key.as_str()))
                .min_by_key(|(_, value)| value.used)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else {
                return Err("scene exceeds the 256 MiB texture budget".into());
            };
            if let Some(value) = self.cache.remove(&key) {
                total -= value.bytes;
            }
        }
        let descriptor = TextureDescriptor::new();
        descriptor.set_texture_type(MTLTextureType::D2);
        descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
        descriptor.set_width(rgba.width() as u64);
        descriptor.set_height(rgba.height() as u64);
        descriptor.set_usage(MTLTextureUsage::ShaderRead);
        descriptor.set_storage_mode(MTLStorageMode::Shared);
        let texture = self.device.new_texture(&descriptor);
        texture.replace_region(
            MTLRegion::new_2d(0, 0, rgba.width() as u64, rgba.height() as u64),
            0,
            rgba.as_ptr().cast(),
            rgba.width() as u64 * 4,
        );
        self.cache.insert(
            name.into(),
            Cached {
                texture,
                bytes: rgba.len(),
                used: self.clock,
            },
        );
        Ok(())
    }
    /// # Safety
    /// Supply the original live CAMetalLayer on the main thread, without concurrent access.
    pub unsafe fn draw(
        &mut self,
        layer: *mut c_void,
        scene: &Scene,
        store: &Store,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        if layer.is_null() || width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        self.clock += 1;
        let active: HashSet<_> = scene.layers.iter().map(|l| l.asset.as_str()).collect();
        for name in &active {
            self.texture(store, name, &active)?;
        }
        let mut total: usize = self.cache.values().map(|v| v.bytes).sum();
        while total > CACHE_LIMIT {
            let oldest = self
                .cache
                .iter()
                .filter(|(key, _)| !active.contains(key.as_str()))
                .min_by_key(|(_, v)| v.used)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else {
                break;
            };
            if let Some(value) = self.cache.remove(&key) {
                total -= value.bytes;
            }
        }
        let layer = MetalLayerRef::from_ptr(layer.cast());
        let Some(drawable) = layer.next_drawable() else {
            return Ok(());
        };
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .ok_or("no Metal attachment")?;
        color.set_texture(Some(drawable.texture()));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_store_action(MTLStoreAction::Store);
        color.set_clear_color(MTLClearColor::new(0.018, 0.024, 0.04, 1.0));
        let command = self.queue.new_command_buffer();
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.pipeline);
        let [x, y, w, h] = content_rect(width, height);
        encoder.set_viewport(MTLViewport {
            originX: x as f64,
            originY: y as f64,
            width: w as f64,
            height: h as f64,
            znear: 0.0,
            zfar: 1.0,
        });
        encoder.set_scissor_rect(MTLScissorRect {
            x: x.ceil() as u64,
            y: y.ceil() as u64,
            width: w.floor() as u64,
            height: h.floor() as u64,
        });
        for layer in &scene.layers {
            let uniform = [
                layer.x / WIDTH * 2.0 - 1.0,
                1.0 - layer.y / HEIGHT * 2.0,
                layer.width / WIDTH * 2.0,
                layer.height / HEIGHT * 2.0,
                layer.opacity,
                0.0,
                0.0,
                0.0,
            ];
            encoder.set_vertex_bytes(
                0,
                std::mem::size_of_val(&uniform) as u64,
                uniform.as_ptr().cast(),
            );
            encoder.set_fragment_texture(0, Some(&self.cache[&layer.asset].texture));
            encoder.draw_primitives(MTLPrimitiveType::TriangleStrip, 0, 4);
        }
        encoder.end_encoding();
        command.present_drawable(drawable);
        command.commit();
        Ok(())
    }
}
