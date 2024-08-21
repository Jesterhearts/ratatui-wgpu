use std::{
    collections::HashMap,
    marker::PhantomData,
    mem::size_of,
    num::NonZeroU64,
};

use palette::{
    rgb::channels,
    Srgb,
};
use ratatui::{
    backend::{
        Backend,
        ClearType,
        WindowSize,
    },
    buffer::Cell,
    layout::{
        Position,
        Size,
    },
    style::Modifier,
};
use rustybuzz::{
    shape_with_plan,
    ttf_parser::GlyphId,
    GlyphBuffer,
    UnicodeBuffer,
};
use tiny_skia::{
    Paint,
    PixmapMut,
    Stroke,
    Transform,
    BYTES_PER_PIXEL,
};
use unicode_width::UnicodeWidthStr;
use wgpu::{
    util::{
        BufferInitDescriptor,
        DeviceExt,
    },
    Buffer,
    BufferUsages,
    CommandEncoderDescriptor,
    Device,
    Extent3d,
    ImageCopyTexture,
    ImageDataLayout,
    IndexFormat,
    LoadOp,
    Operations,
    Origin3d,
    Queue,
    RenderPassColorAttachment,
    RenderPassDescriptor,
    StoreOp,
    Surface,
    SurfaceConfiguration,
    Texture,
    TextureAspect,
};

use crate::{
    backend::{
        build_wgpu_state,
        c2c,
        private::Token,
        PostProcessor,
        RenderSurface,
        RenderTexture,
        TextCachePipeline,
        TextVertexMember,
        Viewport,
        WgpuState,
    },
    fonts::{
        Font,
        Fonts,
    },
    shaders::DefaultPostProcessor,
    utils::{
        plan_cache::PlanCache,
        text_atlas::{
            Atlas,
            Key,
        },
        Outline,
    },
};

const NULL_CELL: Cell = Cell::new("");

/// A ratatui backend leveraging wgpu for rendering.
///
/// Constructed using a [`Builder`](crate::Builder).
///
/// Limitations:
/// - The cursor is tracked but not rendered.
/// - No support for blinking text.
/// - No builtin accessibilty, although [`WgpuBackend::get_text`] is provided to
///   access the screen's contents.
pub struct WgpuBackend<
    'f,
    's,
    P: PostProcessor = DefaultPostProcessor,
    S: RenderSurface<'s> = Surface<'s>,
> {
    pub(super) post_process: P,

    pub(super) cells: Vec<Cell>,
    pub(super) dirty_rows: Vec<bool>,

    pub(super) cursor: (u16, u16),

    pub(super) viewport: Viewport,

    pub(super) surface: S,
    pub(super) _surface: PhantomData<&'s S>,
    pub(super) surface_config: SurfaceConfiguration,
    pub(super) device: Device,
    pub(super) queue: Queue,

    pub(super) plan_cache: PlanCache,
    pub(super) buffer: UnicodeBuffer,

    pub(super) cached: Atlas,
    pub(super) text_cache: Texture,
    pub(super) text_indices: Vec<[u16; 6]>,
    pub(super) text_vertices: Vec<TextVertexMember>,
    pub(super) text_compositor: TextCachePipeline,
    pub(super) text_screen_size_buffer: Buffer,

    pub(super) wgpu_state: WgpuState,

    pub(super) fonts: Fonts<'f>,
    pub(super) reset_fg: Srgb<u8>,
    pub(super) reset_bg: Srgb<u8>,
}

impl<'f, 's, P: PostProcessor, S: RenderSurface<'s>> WgpuBackend<'f, 's, P, S> {
    /// Get the [`PostProcessor`] associated with this backend.
    pub fn post_processor(&self) -> &P {
        &self.post_process
    }

    /// Get a mutable reference to the [`PostProcessor`] associated with this
    /// backend.
    pub fn post_processor_mut(&mut self) -> &mut P {
        &mut self.post_process
    }

    /// Resize the rendering surface. This should be called e.g. to keep the
    /// backend in sync with your window size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let limits = self.device.limits();
        let width = width.min(limits.max_texture_dimension_2d);
        let height = height.min(limits.max_texture_dimension_2d);

        if width == self.surface_config.width && height == self.surface_config.height
            || width == 0
            || height == 0
        {
            return;
        }

        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let dims = self.size().unwrap();
        let current_width = dims.width;
        let current_height = dims.height;

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface
            .configure(&self.device, &self.surface_config, Token);

        let width = width - inset_width;
        let height = height - inset_height;

        let chars_wide = width / self.fonts.width_px();
        let chars_high = height / self.fonts.height_px();

        if chars_high != current_width as u32 || chars_high != current_height as u32 {
            let mut new_buffer = vec![Cell::EMPTY; (chars_high * chars_wide) as usize];
            for (src, dst) in self
                .cells
                .chunks(current_width as usize)
                .zip(new_buffer.chunks_mut(chars_wide as usize))
            {
                let overlap = src.len().min(dst.len());
                dst[..overlap].clone_from_slice(&src[..overlap]);
            }
            self.cells = new_buffer;
        }

        self.dirty_rows.clear();

        self.wgpu_state = build_wgpu_state(
            &self.device,
            chars_wide * self.fonts.width_px(),
            chars_high * self.fonts.height_px(),
        );

        self.post_process.resize(
            &self.device,
            &self.wgpu_state.text_dest_view,
            &self.surface_config,
        );

        info!(
            "Resized from {}x{} to {}x{}",
            current_width, current_height, chars_wide, chars_high,
        );
    }

    /// Get the text currently displayed on the screen.
    pub fn get_text(&self) -> String {
        let bounds = self.size().unwrap();
        self.cells.chunks(bounds.width as usize).fold(
            String::with_capacity((bounds.width + 1) as usize * bounds.height as usize),
            |dest, row| {
                let mut dest = row.iter().fold(dest, |mut dest, s| {
                    dest.push_str(s.symbol());
                    dest
                });
                dest.push('\n');
                dest
            },
        )
    }

    /// Update the fonts used for rendering. This will cause a full repaint of
    /// the screen the next time [`WgpuBackend::flush`] is called.
    pub fn update_fonts(&mut self, new_fonts: Fonts<'f>) {
        self.dirty_rows.clear();
        self.cached.match_fonts(&new_fonts);
        self.fonts = new_fonts;
    }

    fn render(&mut self) {
        let bounds = self.window_size().unwrap();

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Draw Encoder"),
            });

        if !self.text_vertices.is_empty() {
            {
                let mut uniforms = self
                    .queue
                    .write_buffer_with(
                        &self.text_screen_size_buffer,
                        0,
                        NonZeroU64::new(size_of::<[f32; 4]>() as u64).unwrap(),
                    )
                    .unwrap();
                uniforms.copy_from_slice(bytemuck::cast_slice(&[
                    bounds.columns_rows.width as f32 * self.fonts.width_px() as f32,
                    bounds.columns_rows.height as f32 * self.fonts.height_px() as f32,
                    0.0,
                    0.0,
                ]));
            }

            let vertices = self.device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Vertices"),
                contents: bytemuck::cast_slice(&self.text_vertices),
                usage: BufferUsages::VERTEX,
            });

            let indices = self.device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Indices"),
                contents: bytemuck::cast_slice(&self.text_indices),
                usage: BufferUsages::INDEX,
            });

            {
                let mut text_render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("Text Render Pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &self.wgpu_state.text_dest_view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });

                text_render_pass.set_pipeline(&self.text_compositor.pipeline);
                text_render_pass.set_bind_group(0, &self.text_compositor.fs_uniforms, &[]);
                text_render_pass.set_bind_group(1, &self.text_compositor.atlas_bindings, &[]);

                text_render_pass.set_vertex_buffer(0, vertices.slice(..));
                text_render_pass.set_index_buffer(indices.slice(..), IndexFormat::Uint16);
                text_render_pass.draw_indexed(0..self.text_indices.len() as u32 * 6, 0, 0..1);
            }
        }

        let Some(texture) = self.surface.get_current_texture(Token) else {
            return;
        };

        self.post_process.process(
            &mut encoder,
            &self.queue,
            &self.wgpu_state.text_dest_view,
            &self.surface_config,
            texture.get_view(Token),
        );

        self.queue.submit(Some(encoder.finish()));
        texture.present(Token);
    }
}

impl<'f, 's, P: PostProcessor, S: RenderSurface<'s>> Backend for WgpuBackend<'f, 's, P, S> {
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let bounds = self.size()?;

        self.cells
            .resize(bounds.height as usize * bounds.width as usize, Cell::EMPTY);
        self.dirty_rows.resize(bounds.height as usize, true);

        for (x, y, cell) in content {
            self.cells[y as usize * bounds.width as usize + x as usize] = cell.clone();
            self.dirty_rows[y as usize] = true;
        }

        for (row, dirty) in self
            .cells
            .chunks_mut(bounds.width as usize)
            .zip(self.dirty_rows.iter_mut())
        {
            let mut idx = 0;
            loop {
                if idx >= row.len() {
                    break;
                }

                let next = idx + 1 + row[idx].symbol().width().saturating_sub(1);
                for dest in row[idx + 1..next].iter_mut() {
                    if *dest != NULL_CELL {
                        *dest = NULL_CELL;
                        *dirty = true;
                    }
                }

                idx = next;
            }
        }

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<Position> {
        Ok(Position::new(self.cursor.0, self.cursor.1))
    }

    fn set_cursor_position<Pos: Into<Position>>(&mut self, position: Pos) -> std::io::Result<()> {
        let bounds = self.size()?;
        let pos: Position = position.into();
        self.cursor = (pos.x.min(bounds.width - 1), pos.y.min(bounds.height - 1));
        Ok(())
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.cells.clear();
        self.dirty_rows.clear();
        self.cursor = (0, 0);

        Ok(())
    }

    fn size(&self) -> std::io::Result<Size> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };
        let width = self.surface_config.width - inset_width;
        let height = self.surface_config.height - inset_height;

        Ok(Size {
            width: (width / self.fonts.width_px()) as u16,
            height: (height / self.fonts.height_px()) as u16,
        })
    }

    fn window_size(&mut self) -> std::io::Result<WindowSize> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };
        let width = self.surface_config.width - inset_width;
        let height = self.surface_config.height - inset_height;

        Ok(WindowSize {
            columns_rows: Size {
                width: (width / self.fonts.width_px()) as u16,
                height: (height / self.fonts.height_px()) as u16,
            },
            pixels: Size {
                width: width as u16,
                height: height as u16,
            },
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let bounds = self.size()?;
        let mut pending = vec![];

        let mut pending_cache_updates = HashMap::<_, _>::default();

        for (y, row) in self.cells.chunks(bounds.width as usize).enumerate() {
            if !self.dirty_rows[y] {
                continue;
            }

            self.dirty_rows[y] = false;

            let mut x = 0;
            let mut shape =
                |font: &Font, fake_bold, fake_italic, buffer: GlyphBuffer| -> UnicodeBuffer {
                    let metrics = font.font();
                    let advance_scale = self.fonts.height_px() as f32 / metrics.height() as f32;

                    for (info, position) in buffer
                        .glyph_infos()
                        .iter()
                        .zip(buffer.glyph_positions().iter())
                    {
                        let cell = &row[info.cluster as usize];

                        let basey = y as i32 * self.fonts.height_px() as i32
                            + (position.y_offset as f32 * advance_scale) as i32;
                        let basex = x + (position.x_offset as f32 * advance_scale) as i32;
                        let advance = (position.x_advance as f32 * advance_scale) as i32;
                        x += advance;

                        let key = Key {
                            style: cell.modifier.intersection(
                                Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED,
                            ),
                            glyph: info.glyph_id,
                            font: font.id(),
                        };

                        let width = ((metrics
                            .glyph_hor_advance(GlyphId(info.glyph_id as _))
                            .unwrap_or_default() as f32
                            * advance_scale) as u32)
                            .max(font.char_width(self.fonts.height_px()));

                        let cached = self.cached.get(
                            &key,
                            width / font.char_width(self.fonts.height_px()) * self.fonts.width_px(),
                            self.fonts.height_px(),
                        );
                        pending.push((basey, basex, cell.clone(), cached));

                        if cached.cached() {
                            continue;
                        }

                        pending_cache_updates.entry(key).or_insert_with(|| {
                            let mut render = Outline::default();
                            if metrics
                                .outline_glyph(GlyphId(info.glyph_id as _), &mut render)
                                .is_some()
                            {
                                if let Some(path) = render.finish().and_then(|path| {
                                    let skew = if fake_italic {
                                        Transform::from_skew(-0.25, 0.0).post_translate(
                                            -(width as f32 / advance_scale * 0.121),
                                            0.0,
                                        )
                                    } else {
                                        Transform::default()
                                    };

                                    path.transform(
                                        Transform::from_scale(1., -1.)
                                            .post_concat(skew)
                                            .post_translate(0., metrics.ascender() as f32)
                                            .post_scale(advance_scale, advance_scale),
                                    )
                                }) {
                                    return (
                                        cached,
                                        Some((
                                            metrics.ascender(),
                                            metrics
                                                .underline_metrics()
                                                .map(|m| m.thickness)
                                                .unwrap_or(1),
                                            fake_bold,
                                            path,
                                        )),
                                    );
                                }
                            }

                            (cached, None)
                        });
                    }

                    buffer.clear()
                };

            let mut current_font = self.fonts.last_resort();
            let mut current_fake_bold = false;
            let mut current_fake_italic = false;

            for (cluster, cell) in row.iter().enumerate() {
                let (font, fake_bold, fake_italic) = self.fonts.font_for_cell(cell);

                if font.id() != current_font.id()
                    || current_fake_bold != fake_bold
                    || current_fake_italic != fake_italic
                {
                    let mut buffer = std::mem::take(&mut self.buffer);

                    self.buffer = shape(
                        current_font,
                        current_fake_bold,
                        current_fake_italic,
                        shape_with_plan(
                            current_font.font(),
                            self.plan_cache.get(current_font, &mut buffer),
                            buffer,
                        ),
                    );

                    current_font = font;
                    current_fake_bold = fake_bold;
                    current_fake_italic = fake_italic;
                }

                for ch in cell.symbol().chars() {
                    self.buffer.add(ch, cluster as u32);
                }
            }

            let mut buffer = std::mem::take(&mut self.buffer);
            self.buffer = shape(
                current_font,
                current_fake_bold,
                current_fake_italic,
                shape_with_plan(
                    current_font.font(),
                    self.plan_cache.get(current_font, &mut buffer),
                    buffer,
                ),
            );
        }

        for (key, (cached, maybe_path)) in pending_cache_updates {
            let mut image =
                vec![[0; BYTES_PER_PIXEL]; cached.width as usize * cached.height as usize];

            if let Some((underline_position, underline_thickness, fake_bold, path)) = maybe_path {
                let mut pixmap = PixmapMut::from_bytes(
                    bytemuck::cast_slice_mut(&mut image),
                    cached.width,
                    cached.height,
                )
                .expect("Invalid image buffer");

                let mut paint = Paint::default();
                paint.set_color_rgba8(255, 255, 255, 255);
                pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    Transform::default(),
                    None,
                );

                if fake_bold {
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &Stroke {
                            width: 1.5,
                            ..Default::default()
                        },
                        Transform::default(),
                        None,
                    );
                }

                if key.style.contains(Modifier::UNDERLINED) {
                    for y in underline_position..underline_position + underline_thickness {
                        for x in 0..cached.width {
                            image[y as usize * cached.width as usize + x as usize] =
                                [255; BYTES_PER_PIXEL];
                        }
                    }
                }
            }

            self.queue.write_texture(
                ImageCopyTexture {
                    texture: &self.text_cache,
                    mip_level: 0,
                    origin: Origin3d {
                        x: cached.x,
                        y: cached.y,
                        z: 0,
                    },
                    aspect: TextureAspect::All,
                },
                &image.into_iter().map(|[_, _, _, a]| a).collect::<Vec<_>>(),
                ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width),
                    rows_per_image: Some(cached.height),
                },
                Extent3d {
                    width: cached.width,
                    height: cached.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        if self.post_process.needs_update() || !pending.is_empty() {
            self.text_vertices.clear();
            self.text_indices.clear();

            let mut index_offset = 0u16;
            for (y, x, cell, cached) in pending {
                let reverse = cell.modifier.contains(Modifier::REVERSED);
                let fg_color = c2c(cell.fg, self.reset_fg);
                let bg_color = c2c(cell.bg, self.reset_bg);

                let (mut fg_color, bg_color) = if reverse {
                    (bg_color, fg_color)
                } else {
                    (fg_color, bg_color)
                };

                if cell.modifier.contains(Modifier::DIM) {
                    use palette::Lighten;
                    fg_color = fg_color.into_format::<f32>().lighten(0.5).into_format();
                }

                let fg_color: u32 = fg_color.into_u32::<channels::Rgba>();
                let bg_color: u32 = bg_color.into_u32::<channels::Rgba>();

                for offset_y in (0..cached.height).step_by(self.fonts.height_px() as usize) {
                    for offset_x in (0..cached.width).step_by(self.fonts.width_px() as usize) {
                        self.text_indices.push([
                            index_offset,     // x, y
                            index_offset + 1, // x + w, y
                            index_offset + 2, // x, y + h
                            index_offset + 2, // x, y + h
                            index_offset + 3, // x + w, y + h
                            index_offset + 1, // x + w y
                        ]);
                        index_offset += 4;

                        let x = x as f32 + offset_x as f32;
                        let y = y as f32 + offset_y as f32;
                        let uvx = cached.x + offset_x;
                        let uvy = cached.y + offset_y;
                        // 0
                        self.text_vertices.push(TextVertexMember {
                            vertex: [x, y],
                            uv: [uvx as f32, uvy as f32],
                            fg_color,
                            bg_color,
                        });
                        // 1
                        self.text_vertices.push(TextVertexMember {
                            vertex: [x + self.fonts.width_px() as f32, y],
                            uv: [uvx as f32 + self.fonts.width_px() as f32, uvy as f32],
                            fg_color,
                            bg_color,
                        });
                        // 2
                        self.text_vertices.push(TextVertexMember {
                            vertex: [x, y + self.fonts.height_px() as f32],
                            uv: [uvx as f32, uvy as f32 + self.fonts.height_px() as f32],
                            fg_color,
                            bg_color,
                        });
                        // 3
                        self.text_vertices.push(TextVertexMember {
                            vertex: [
                                x + self.fonts.width_px() as f32,
                                y + self.fonts.height_px() as f32,
                            ],
                            uv: [
                                uvx as f32 + self.fonts.width_px() as f32,
                                uvy as f32 + self.fonts.height_px() as f32,
                            ],
                            fg_color,
                            bg_color,
                        });
                    }
                }
            }

            self.render();
        }

        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> std::io::Result<()> {
        let bounds = self.size()?;
        let line_start = self.cursor.1 as usize * bounds.width as usize;
        let idx = line_start + self.cursor.0 as usize;

        match clear_type {
            ClearType::All => self.clear(),
            ClearType::AfterCursor => {
                self.cells.truncate(idx + 1);
                Ok(())
            }
            ClearType::BeforeCursor => {
                self.cells[..idx].fill(Cell::EMPTY);
                Ok(())
            }
            ClearType::CurrentLine => {
                self.cells[line_start..line_start + bounds.width as usize].fill(Cell::EMPTY);
                Ok(())
            }
            ClearType::UntilNewLine => {
                let remain = (bounds.width - self.cursor.0) as usize;
                self.cells[idx..idx + remain].fill(Cell::EMPTY);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use image::{
        load_from_memory,
        GenericImageView,
        ImageBuffer,
        Rgba,
    };
    use ratatui::{
        widgets::{
            Block,
            Paragraph,
        },
        Terminal,
    };
    use serial_test::serial;
    use wgpu::{
        CommandEncoderDescriptor,
        Device,
        Extent3d,
        ImageCopyBuffer,
        ImageDataLayout,
        Queue,
    };

    use crate::{
        backend::HeadlessSurface,
        shaders::DefaultPostProcessor,
        Builder,
        Font,
    };

    fn tex2buffer(device: &Device, queue: &Queue, surface: &HeadlessSurface) {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            surface.texture.as_ref().unwrap().as_image_copy(),
            ImageCopyBuffer {
                buffer: surface.buffer.as_ref().unwrap(),
                layout: ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(surface.buffer_width),
                    rows_per_image: Some(surface.height),
                },
            },
            Extent3d {
                width: surface.width,
                height: surface.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
    }

    #[test]
    #[serial]
    fn a_z() {
        let mut terminal = Terminal::new(
            futures_lite::future::block_on(
                Builder::<DefaultPostProcessor>::from_font(
                    Font::new(include_bytes!("fonts/CascadiaMono-Regular.ttf"))
                        .expect("Invalid font file"),
                )
                .with_dimensions(NonZeroU32::new(72).unwrap(), NonZeroU32::new(512).unwrap())
                .build_headless(),
            )
            .unwrap(),
        )
        .unwrap();

        terminal
            .draw(|f| {
                let block = Block::bordered();
                let area = block.inner(f.area());
                f.render_widget(block, f.area());
                f.render_widget(Paragraph::new("ABCDEFGHIJKLMNOPQRSTUVWXYZ"), area);
            })
            .unwrap();

        let surface = &terminal.backend().surface;
        tex2buffer(
            &terminal.backend().device,
            &terminal.backend().queue,
            surface,
        );
        {
            let buffer = surface.buffer.as_ref().unwrap().slice(..);

            let (send, recv) = oneshot::channel();
            buffer.map_async(wgpu::MapMode::Read, move |data| {
                send.send(data).unwrap();
            });
            terminal.backend().device.poll(wgpu::MaintainBase::Wait);
            recv.recv().unwrap().unwrap();

            let data = buffer.get_mapped_range();
            let image =
                ImageBuffer::<Rgba<u8>, _>::from_raw(surface.width, surface.height, data).unwrap();

            let pixels = image.pixels().copied().collect::<Vec<_>>();
            let golden = load_from_memory(include_bytes!("goldens/a_z.png")).unwrap();
            let golden_pixels = golden.pixels().map(|(_, _, px)| px).collect::<Vec<_>>();

            assert!(
                pixels == golden_pixels,
                "Rendered image differs from golden"
            );
        }

        surface.buffer.as_ref().unwrap().unmap();
    }

    #[test]
    #[serial]
    fn arabic() {
        let mut terminal = Terminal::new(
            futures_lite::future::block_on(
                Builder::<DefaultPostProcessor>::from_font(
                    Font::new(include_bytes!("fonts/CascadiaMono-Regular.ttf"))
                        .expect("Invalid font file"),
                )
                .with_dimensions(NonZeroU32::new(72).unwrap(), NonZeroU32::new(256).unwrap())
                .build_headless(),
            )
            .unwrap(),
        )
        .unwrap();

        terminal
            .draw(|f| {
                let block = Block::bordered();
                let area = block.inner(f.area());
                f.render_widget(block, f.area());
                f.render_widget(Paragraph::new("مرحبا بالعالم"), area);
            })
            .unwrap();

        let surface = &terminal.backend().surface;
        tex2buffer(
            &terminal.backend().device,
            &terminal.backend().queue,
            surface,
        );
        {
            let buffer = surface.buffer.as_ref().unwrap().slice(..);

            let (send, recv) = oneshot::channel();
            buffer.map_async(wgpu::MapMode::Read, move |data| {
                send.send(data).unwrap();
            });
            terminal.backend().device.poll(wgpu::MaintainBase::Wait);
            recv.recv().unwrap().unwrap();

            let data = buffer.get_mapped_range();
            let image =
                ImageBuffer::<Rgba<u8>, _>::from_raw(surface.width, surface.height, data).unwrap();

            let pixels = image.pixels().copied().collect::<Vec<_>>();
            let golden = load_from_memory(include_bytes!("goldens/arabic.png")).unwrap();
            let golden_pixels = golden.pixels().map(|(_, _, px)| px).collect::<Vec<_>>();

            assert!(
                pixels == golden_pixels,
                "Rendered image differs from golden"
            );
        }

        surface.buffer.as_ref().unwrap().unmap();
    }

    #[test]
    #[serial]
    fn really_wide() {
        let mut terminal = Terminal::new(
            futures_lite::future::block_on(
                Builder::<DefaultPostProcessor>::from_font(
                    Font::new(include_bytes!("fonts/Fairfax.ttf")).expect("Invalid font file"),
                )
                .with_dimensions(NonZeroU32::new(72).unwrap(), NonZeroU32::new(512).unwrap())
                .build_headless(),
            )
            .unwrap(),
        )
        .unwrap();

        terminal
            .draw(|f| {
                let block = Block::bordered();
                let area = block.inner(f.area());
                f.render_widget(block, f.area());
                f.render_widget(Paragraph::new("Ｈｅｌｌｏ, ｗｏｒｌｄ!"), area);
            })
            .unwrap();

        let surface = &terminal.backend().surface;
        tex2buffer(
            &terminal.backend().device,
            &terminal.backend().queue,
            surface,
        );
        {
            let buffer = surface.buffer.as_ref().unwrap().slice(..);

            let (send, recv) = oneshot::channel();
            buffer.map_async(wgpu::MapMode::Read, move |data| {
                send.send(data).unwrap();
            });
            terminal.backend().device.poll(wgpu::MaintainBase::Wait);
            recv.recv().unwrap().unwrap();

            let data = buffer.get_mapped_range();
            let image =
                ImageBuffer::<Rgba<u8>, _>::from_raw(surface.width, surface.height, data).unwrap();

            let pixels = image.pixels().copied().collect::<Vec<_>>();
            let golden = load_from_memory(include_bytes!("goldens/really_wide.png")).unwrap();
            let golden_pixels = golden.pixels().map(|(_, _, px)| px).collect::<Vec<_>>();

            assert!(
                pixels == golden_pixels,
                "Rendered image differs from golden"
            );
        }

        surface.buffer.as_ref().unwrap().unmap();
    }
}
