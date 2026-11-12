use crate::animation::{self, AtlasAnimation, Frame, Style};
use crate::model::{content_rect, Layer, Scene, HEIGHT, WIDTH};
use crate::xp3::{Assets, Store};
use foreign_types::ForeignTypeRef;
use metal::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::io::Cursor;

const SHADER: &str = include_str!("scene.metal");
const CACHE_LIMIT: usize = 160 * 1024 * 1024;
const TEXTURE_LIMIT: usize = 256 * 1024 * 1024;
const DECODE_LIMIT: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum TextureKey {
    Asset(String),
    Composite(Vec<String>),
}
struct DrawLayer {
    layer: Layer,
    key: TextureKey,
}

struct Cached {
    frames: Vec<([u32; 4], Texture)>,
    animation: Option<AtlasAnimation>,
    initial_clip: [u32; 4],
    source_size: [u32; 2],
    bytes: usize,
    used: u64,
}
impl Cached {
    fn frame(&self, time: u64) -> (&TextureRef, [u32; 4]) {
        let rect = self
            .animation
            .as_ref()
            .map_or(self.frames[0].0, |a| a.clip_at(time));
        let texture = &self
            .frames
            .iter()
            .find(|f| f.0 == rect)
            .expect("validated atlas rectangle")
            .1;
        (texture, rect)
    }
}
pub struct Renderer {
    device: Device,
    queue: CommandQueue,
    pipeline: RenderPipelineState,
    composition: RenderPipelineState,
    canvases: [Texture; 2],
    white: Texture,
    cache: HashMap<TextureKey, Cached>,
    clock: u64,
}

fn texture(
    device: &DeviceRef,
    width: u32,
    height: u32,
    format: MTLPixelFormat,
    render: bool,
) -> Texture {
    let d = TextureDescriptor::new();
    d.set_texture_type(MTLTextureType::D2);
    d.set_pixel_format(format);
    d.set_width(width as u64);
    d.set_height(height as u64);
    d.set_usage(if render {
        MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget
    } else {
        MTLTextureUsage::ShaderRead
    });
    d.set_storage_mode(if render {
        MTLStorageMode::Private
    } else {
        MTLStorageMode::Shared
    });
    device.new_texture(&d)
}
impl Renderer {
    /// # Safety
    /// Supply a live CAMetalLayer with a Metal device on the main thread.
    pub unsafe fn new(layer: *mut c_void) -> Result<Self, String> {
        if layer.is_null() {
            return Err("null CAMetalLayer".into());
        }
        let layer = MetalLayerRef::from_ptr(layer.cast());
        Self::for_device(layer.device().to_owned())
    }
    /// Headless construction also lets module checks exercise real Metal without UI.
    pub fn for_device(device: Device) -> Result<Self, String> {
        let queue = device.new_command_queue();
        let library = device.new_library_with_source(SHADER, &CompileOptions::new())?;
        let make =
            |vertex: &str, fragment: &str, blend: bool| -> Result<RenderPipelineState, String> {
                let d = RenderPipelineDescriptor::new();
                let vertex = library.get_function(vertex, None)?;
                let fragment = library.get_function(fragment, None)?;
                d.set_vertex_function(Some(&vertex));
                d.set_fragment_function(Some(&fragment));
                let a = d
                    .color_attachments()
                    .object_at(0)
                    .ok_or("no Metal color attachment")?;
                a.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                a.set_blending_enabled(blend);
                a.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
                a.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
                a.set_source_alpha_blend_factor(MTLBlendFactor::One);
                a.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
                device.new_render_pipeline_state(&d)
            };
        let pipeline = make("quad_vertex", "quad_fragment", true)?;
        let composition = make("fullscreen_vertex", "composite_fragment", false)?;
        let canvases = [
            texture(
                &device,
                WIDTH as u32,
                HEIGHT as u32,
                MTLPixelFormat::BGRA8Unorm,
                true,
            ),
            texture(
                &device,
                WIDTH as u32,
                HEIGHT as u32,
                MTLPixelFormat::BGRA8Unorm,
                true,
            ),
        ];
        let white = texture(&device, 1, 1, MTLPixelFormat::RGBA8Unorm, false);
        white.replace_region(
            MTLRegion::new_2d(0, 0, 1, 1),
            0,
            [255u8; 4].as_ptr().cast(),
            4,
        );
        Ok(Self {
            device,
            queue,
            pipeline,
            composition,
            canvases,
            white,
            cache: HashMap::new(),
            clock: 0,
        })
    }

    fn evict(
        &mut self,
        active: &HashSet<TextureKey>,
        incoming: usize,
        limit: usize,
    ) -> Result<(), String> {
        let mut total: usize = self.cache.values().map(|v| v.bytes).sum();
        while total.saturating_add(incoming) > limit {
            let key = self
                .cache
                .iter()
                .filter(|(k, _)| !active.contains(*k))
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone());
            let Some(key) = key else {
                if limit == CACHE_LIMIT {
                    break;
                }
                return Err("scene exceeds the 256 MiB texture budget".into());
            };
            total -= self.cache.remove(&key).expect("cache entry exists").bytes;
        }
        Ok(())
    }
    fn load(
        &mut self,
        store: &Store,
        name: &str,
        active: &HashSet<TextureKey>,
        initial: Option<[u32; 4]>,
        allow_asd: bool,
    ) -> Result<(), String> {
        let key = TextureKey::Asset(name.into());
        if let Some(c) = self.cache.get_mut(&key) {
            if c.animation.is_some() && initial.is_none() {
                return Err("ASD requires explicit atlas_start clip rectangle".into());
            }
            if initial.is_none_or(|r| r == c.initial_clip) {
                c.used = self.clock;
                return Ok(());
            }
        }
        self.cache.remove(&key);
        let bytes = store.bytes(name).map_err(|e| e.to_string())?;
        let (width, height) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        let decoded = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or("image dimensions overflow")?;
        if width == 0 || height == 0 || height > 8192 || decoded > DECODE_LIMIT {
            return Err("image exceeds bounded decode budget".into());
        }
        let sidecar = name.rsplit_once('.').map(|(s, _)| format!("{s}.asd"));
        let animation = if allow_asd {
            match sidecar.as_ref().and_then(|n| store.find(n, &["asd"]).ok()) {
                Some(asd) => {
                    let a = AtlasAnimation::parse(
                        &store.text(&asd).map_err(|e| e.to_string())?,
                        initial.ok_or("ASD requires explicit atlas_start clip rectangle")?,
                    )?;
                    a.validate_dimensions(width, height)?;
                    Some(a)
                }
                None => None,
            }
        } else {
            None
        };
        // Wide images are admitted only as validated animation sheets. No wide
        // GPU texture is ever allocated; each distinct visible clip is uploaded.
        if width > 8192 && (width > 32768 || animation.is_none()) {
            return Err("wide image requires a bounded ASD clip timeline".into());
        }
        let rectangles: Vec<[u32; 4]> = if let Some(a) = &animation {
            let mut r = Vec::new();
            for f in &a.frames {
                if !r.contains(&f.clip) {
                    r.push(f.clip);
                }
            }
            r
        } else {
            vec![[0, 0, width, height]]
        };
        let incoming = rectangles
            .iter()
            .try_fold(0usize, |s, r| {
                s.checked_add(r[2] as usize * r[3] as usize * 4)
            })
            .ok_or("atlas texture size overflow")?;
        self.evict(active, incoming, TEXTURE_LIMIT)?;
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(if animation.is_some() { 32768 } else { 8192 });
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(DECODE_LIMIT as u64);
        reader.limits(limits);
        let rgba = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        let mut frames = Vec::new();
        for rect in rectangles {
            let [x, y, w, h] = rect;
            let tex = texture(&self.device, w, h, MTLPixelFormat::RGBA8Unorm, false);
            // Upload rows directly from the CPU image with its original stride;
            // there is no second full-sheet copy and no oversized GPU allocation.
            let offset = ((y as usize * width as usize) + x as usize) * 4;
            tex.replace_region(
                MTLRegion::new_2d(0, 0, w as u64, h as u64),
                0,
                unsafe { rgba.as_ptr().add(offset).cast() },
                width as u64 * 4,
            );
            frames.push((rect, tex));
        }
        self.cache.insert(
            key,
            Cached {
                frames,
                animation,
                initial_clip: initial.unwrap_or([0, 0, width, height]),
                source_size: [width, height],
                bytes: incoming,
                used: self.clock,
            },
        );
        Ok(())
    }

    fn draw_quad(
        encoder: &RenderCommandEncoderRef,
        tex: &TextureRef,
        rect: [f32; 4],
        opacity: f32,
        premultiplied: bool,
        style: &Style,
    ) {
        let uniform = [
            rect,
            [
                opacity.clamp(0.0, 1.0),
                if style.grayscale { 1.0 } else { 0.0 },
                if premultiplied { 1.0 } else { 0.0 },
                0.0,
            ],
            [style.gamma[0], style.gamma[1], style.gamma[2], 0.0],
            [style.floor[0], style.floor[1], style.floor[2], 0.0],
            [style.ceiling[0], style.ceiling[1], style.ceiling[2], 0.0],
            style.tint,
        ];
        encoder.set_vertex_bytes(
            0,
            std::mem::size_of_val(&uniform) as u64,
            uniform.as_ptr().cast(),
        );
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of_val(&uniform) as u64,
            uniform.as_ptr().cast(),
        );
        encoder.set_fragment_texture(0, Some(tex));
        encoder.draw_primitives(MTLPrimitiveType::TriangleStrip, 0, 4);
    }

    fn prepare_layers(
        &mut self,
        command: &CommandBufferRef,
        layers: &[Layer],
        active: &mut HashSet<TextureKey>,
    ) -> Result<Vec<DrawLayer>, String> {
        let mut result = Vec::new();
        let mut start = 0;
        while start < layers.len() {
            let first = &layers[start];
            let end = start
                + layers[start..]
                    .iter()
                    .take_while(|l| l.group == first.group)
                    .count();
            let group = &layers[start..end];
            let base = &self.cache[&TextureKey::Asset(first.asset.clone())];
            let [width, height] = base.source_size;
            let scale = [first.width / width as f32, first.height / height as f32];
            let composable = first.group != 0
                && group.len() > 1
                && scale.iter().all(|s| s.is_finite() && *s > 0.0)
                && group.iter().all(|l| {
                    let source = &self.cache[&TextureKey::Asset(l.asset.clone())];
                    source.animation.is_none()
                        && source.source_size[0] <= width
                        && source.source_size[1] <= height
                        && l.x == first.x
                        && l.y == first.y
                        && l.opacity == first.opacity
                        && (l.width - source.source_size[0] as f32 * scale[0]).abs() < 0.01
                        && (l.height - source.source_size[1] as f32 * scale[1]).abs() < 0.01
                });
            if composable {
                // Composite native cutouts before any subpixel motion or scaling.
                let key = TextureKey::Composite(group.iter().map(|l| l.asset.clone()).collect());
                active.insert(key.clone());
                if let Some(cached) = self.cache.get_mut(&key) {
                    cached.used = self.clock;
                } else {
                    let bytes = width as usize * height as usize * 4;
                    self.evict(active, bytes, TEXTURE_LIMIT)?;
                    let tex = texture(
                        &self.device,
                        width,
                        height,
                        MTLPixelFormat::BGRA8Unorm,
                        true,
                    );
                    let pass = RenderPassDescriptor::new();
                    let color = pass
                        .color_attachments()
                        .object_at(0)
                        .ok_or("no Metal attachment")?;
                    color.set_texture(Some(&tex));
                    color.set_load_action(MTLLoadAction::Clear);
                    color.set_store_action(MTLStoreAction::Store);
                    color.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
                    let encoder = command.new_render_command_encoder(pass);
                    encoder.set_render_pipeline_state(&self.pipeline);
                    encoder.set_viewport(MTLViewport {
                        originX: 0.0,
                        originY: 0.0,
                        width: width as f64,
                        height: height as f64,
                        znear: 0.0,
                        zfar: 1.0,
                    });
                    for l in group {
                        let source = &self.cache[&TextureKey::Asset(l.asset.clone())];
                        let (image, _) = source.frame(0);
                        Self::draw_quad(
                            encoder,
                            image,
                            [
                                -1.0,
                                1.0,
                                source.source_size[0] as f32 / width as f32 * 2.0,
                                source.source_size[1] as f32 / height as f32 * 2.0,
                            ],
                            1.0,
                            false,
                            &Style::default(),
                        );
                    }
                    encoder.end_encoding();
                    let clip = [0, 0, width, height];
                    self.cache.insert(
                        key.clone(),
                        Cached {
                            frames: vec![(clip, tex)],
                            animation: None,
                            initial_clip: clip,
                            source_size: [width, height],
                            bytes,
                            used: self.clock,
                        },
                    );
                }
                result.push(DrawLayer {
                    layer: first.clone(),
                    key,
                });
            } else {
                result.extend(group.iter().map(|l| DrawLayer {
                    layer: l.clone(),
                    key: TextureKey::Asset(l.asset.clone()),
                }));
            }
            start = end;
        }
        Ok(result)
    }

    fn encode_layers(
        &self,
        command: &CommandBufferRef,
        target: &TextureRef,
        layers: &[DrawLayer],
        styles: &BTreeMap<u32, Style>,
        frame: &Frame,
        elapsed: u64,
    ) -> Result<(), String> {
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .ok_or("no Metal attachment")?;
        color.set_texture(Some(target));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_store_action(MTLStoreAction::Store);
        color.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 1.0));
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.pipeline);
        encoder.set_viewport(MTLViewport {
            originX: 0.0,
            originY: 0.0,
            width: WIDTH as f64,
            height: HEIGHT as f64,
            znear: 0.0,
            zfar: 1.0,
        });
        for DrawLayer { layer: l, key } in layers {
            if ![l.x, l.y, l.width, l.height, l.opacity]
                .iter()
                .all(|v| v.is_finite())
                || l.width < 0.0
                || l.height < 0.0
            {
                encoder.end_encoding();
                return Err("invalid layer geometry".into());
            }
            let cached = self.cache.get(key).ok_or("texture was not loaded")?;
            let (tex, clip) =
                cached.frame(frame.atlas_times.get(&l.asset).copied().unwrap_or(elapsed));
            let mut width = l.width;
            let mut height = l.height;
            if cached.animation.is_some() {
                // An un-clipped source layer retains the sheet dimensions in
                // older saves; only its visible viewport is drawn as a frame.
                if width == cached.source_size[0] as f32 {
                    width = clip[2] as f32;
                }
                if height == cached.source_size[1] as f32 {
                    height = clip[3] as f32;
                }
            }
            let style = styles.get(&l.group).cloned().unwrap_or_default();
            Self::draw_quad(
                encoder,
                tex,
                [
                    l.x / WIDTH * 2.0 - 1.0,
                    1.0 - l.y / HEIGHT * 2.0,
                    width / WIDTH * 2.0,
                    height / HEIGHT * 2.0,
                ],
                l.opacity,
                matches!(key, TextureKey::Composite(_)),
                &style,
            );
        }
        encoder.end_encoding();
        Ok(())
    }

    fn encode(
        &mut self,
        scene: &Scene,
        store: &Store,
        target: &TextureRef,
    ) -> Result<CommandBuffer, String> {
        let result = self.encode_frame(scene, store, target);
        if result.is_err() {
            self.cache.retain(|key, cached| {
                !matches!(key, TextureKey::Composite(_)) || cached.used != self.clock
            });
        }
        result
    }

    fn encode_frame(
        &mut self,
        scene: &Scene,
        store: &Store,
        target: &TextureRef,
    ) -> Result<CommandBuffer, String> {
        let frame = animation::frame(scene)?;
        self.clock = self.clock.wrapping_add(1);
        let mut active: HashSet<TextureKey> = frame
            .current
            .iter()
            .chain(&frame.previous)
            .map(|l| TextureKey::Asset(l.asset.clone()))
            .collect();
        let rule = if frame.kind == 1 && frame.progress < 1.0 {
            Some(
                store
                    .find(&frame.rule, &["webp", "png"])
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if let Some(r) = &rule {
            active.insert(TextureKey::Asset(r.clone()));
        }
        for key in &active {
            let TextureKey::Asset(name) = key else {
                continue;
            };
            self.load(
                store,
                name,
                &active,
                frame.atlas_clips.get(name).copied(),
                rule.as_ref() != Some(name),
            )?;
        }
        let command = self.queue.new_command_buffer().to_owned();
        let current = self.prepare_layers(&command, &frame.current, &mut active)?;
        let previous = if frame.progress < 1.0 {
            self.prepare_layers(&command, &frame.previous, &mut active)?
        } else {
            Vec::new()
        };
        self.evict(&active, 0, CACHE_LIMIT)?;
        self.encode_layers(
            &command,
            &self.canvases[1],
            &current,
            &frame.styles,
            &frame,
            scene.visual.elapsed_ms,
        )?;
        if frame.progress < 1.0 {
            self.encode_layers(
                &command,
                &self.canvases[0],
                &previous,
                &frame.previous_styles,
                &frame,
                scene.visual.elapsed_ms,
            )?;
        }
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .ok_or("no Metal attachment")?;
        color.set_texture(Some(target));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_store_action(MTLStoreAction::Store);
        color.set_clear_color(MTLClearColor::new(0.018, 0.024, 0.04, 1.0));
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.composition);
        let [x, y, w, h] = content_rect(target.width() as f32, target.height() as f32);
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
        let uniform = [
            frame.progress,
            frame.kind as f32,
            frame.vague,
            0.0,
            frame.zoom[0],
            frame.zoom[1],
            0.0,
            0.0,
        ];
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of_val(&uniform) as u64,
            uniform.as_ptr().cast(),
        );
        encoder.set_fragment_texture(
            0,
            Some(if frame.progress < 1.0 {
                &self.canvases[0]
            } else {
                &self.canvases[1]
            }),
        );
        encoder.set_fragment_texture(1, Some(&self.canvases[1]));
        encoder.set_fragment_texture(
            2,
            Some(rule.as_ref().map_or(&*self.white, |r| {
                self.cache[&TextureKey::Asset(r.clone())].frame(0).0
            })),
        );
        encoder.draw_primitives(MTLPrimitiveType::TriangleStrip, 0, 4);
        encoder.end_encoding();
        Ok(command)
    }

    /// Render to a caller-owned BGRA8 texture and wait for completion. This is
    /// useful for offline screenshots/tests and does not create any window.
    pub fn render_to_texture(
        &mut self,
        scene: &Scene,
        store: &Store,
        target: &TextureRef,
    ) -> Result<(), String> {
        if target.pixel_format() != MTLPixelFormat::BGRA8Unorm
            || target.width() == 0
            || target.height() == 0
        {
            return Err("offscreen target must be a nonempty BGRA8 texture".into());
        }
        let command = self.encode(scene, store, target)?;
        command.commit();
        command.wait_until_completed();
        if command.status() == MTLCommandBufferStatus::Error {
            return Err("Metal offscreen command failed".into());
        }
        Ok(())
    }

    /// Resident GPU asset bytes and largest uploaded frame dimension. The two
    /// fixed 2560x1440 scene targets are separate (29,491,200 bytes total).
    pub fn cache_usage(&self) -> (usize, u64) {
        (
            self.cache.values().map(|c| c.bytes).sum(),
            self.cache
                .values()
                .flat_map(|c| c.frames.iter().map(|(_, t)| t.width().max(t.height())))
                .max()
                .unwrap_or(0),
        )
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
        if layer.is_null()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            return Ok(());
        }
        let layer = MetalLayerRef::from_ptr(layer.cast());
        let Some(drawable) = layer.next_drawable() else {
            return Ok(());
        };
        let command = self.encode(scene, store, drawable.texture())?;
        command.present_drawable(drawable);
        command.commit();
        Ok(())
    }
}
