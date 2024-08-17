use std::{
    collections::HashMap,
    mem::size_of,
    num::{
        NonZeroU32,
        NonZeroU64,
    },
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
    style::{
        Color,
        Modifier,
    },
};
use swash::{
    scale::{
        Render,
        ScaleContext,
        Source,
        StrikeWith,
    },
    shape::{
        cluster::GlyphCluster,
        ShapeContext,
    },
    text::{
        cluster::{
            CharCluster,
            Parser,
            Token,
        },
        Codepoint,
        Script,
    },
    zeno::{
        Angle,
        Transform,
    },
};
use unicode_width::UnicodeWidthStr;
use wgpu::{
    include_wgsl,
    util::{
        BufferInitDescriptor,
        DeviceExt,
    },
    vertex_attr_array,
    AddressMode,
    Backends,
    BindGroup,
    BindGroupDescriptor,
    BindGroupEntry,
    BindGroupLayoutDescriptor,
    BindGroupLayoutEntry,
    BindingResource,
    BindingType,
    Buffer,
    BufferBindingType,
    BufferDescriptor,
    BufferUsages,
    ColorTargetState,
    ColorWrites,
    CommandEncoder,
    CommandEncoderDescriptor,
    Device,
    Extent3d,
    FilterMode,
    FragmentState,
    ImageCopyTexture,
    ImageDataLayout,
    IndexFormat,
    Instance,
    InstanceDescriptor,
    InstanceFlags,
    Limits,
    LoadOp,
    MultisampleState,
    Operations,
    Origin3d,
    PipelineCompilationOptions,
    PipelineLayoutDescriptor,
    PresentMode,
    PrimitiveState,
    PrimitiveTopology,
    Queue,
    RenderPassColorAttachment,
    RenderPassDescriptor,
    RenderPipeline,
    RenderPipelineDescriptor,
    Sampler,
    SamplerBindingType,
    SamplerDescriptor,
    ShaderStages,
    StoreOp,
    Surface,
    SurfaceConfiguration,
    SurfaceTarget,
    Texture,
    TextureAspect,
    TextureDescriptor,
    TextureDimension,
    TextureFormat,
    TextureSampleType,
    TextureUsages,
    TextureView,
    TextureViewDescriptor,
    TextureViewDimension,
    VertexBufferLayout,
    VertexState,
    VertexStepMode,
};

use crate::{
    shaders::DefaultPostProcessor,
    utils::text_atlas::{
        Atlas,
        Key,
    },
    Error,
    Font,
    Fonts,
    Result,
};

const CACHE_WIDTH: u32 = 1800;
const CACHE_HEIGHT: u32 = 1200;

const NULL_CELL: Cell = Cell::new("");

fn c2c(color: ratatui::style::Color, reset: Srgb<u8>) -> Srgb<u8> {
    match color {
        Color::Reset => reset,
        Color::Black => palette::named::BLACK,
        Color::Red => palette::named::RED,
        Color::Green => palette::named::GREEN,
        Color::Yellow => palette::named::YELLOW,
        Color::Blue => palette::named::BLUE,
        Color::Magenta => palette::named::MAGENTA,
        Color::Cyan => palette::named::CYAN,
        Color::Gray => palette::named::GRAY,
        Color::DarkGray => palette::named::DARKGRAY,
        Color::LightRed => Srgb::new(240, 128, 128),
        Color::LightGreen => palette::named::LIGHTGREEN,
        Color::LightYellow => palette::named::LIGHTYELLOW,
        Color::LightBlue => palette::named::LIGHTBLUE,
        Color::LightMagenta => Srgb::new(255, 128, 255),
        Color::LightCyan => palette::named::LIGHTCYAN,
        Color::White => palette::named::WHITE,
        Color::Rgb(r, g, b) => Srgb::new(r, g, b),
        Color::Indexed(idx) => {
            let rgb = coolor::AnsiColor::new(idx).to_rgb();
            Srgb::new(rgb.r, rgb.g, rgb.b)
        }
    }
}

// Vertex + UVCoord + Color
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct TextVertexMember {
    vertex: [f32; 2],
    uv: [f32; 2],
    fg_color: u32,
    bg_color: u32,
}

struct TextCachePipeline {
    pipeline: RenderPipeline,
    fs_uniforms: BindGroup,
    atlas_bindings: BindGroup,
}

struct WgpuState {
    text_dest_view: TextureView,
}

/// A pipeline for post-processing rendered text.
pub trait PostProcessor {
    /// Custom user data which will be supplied during creation of the post
    /// processor. Use this to pass in any external state your processor
    /// requires.
    type UserData;

    /// Called during initialization of the backend. This should fully
    /// initialize the post processor for rendering. Note that you are expected
    /// to render to the final surface during [`PostProcessor::process`].
    fn compile(
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        user_data: Self::UserData,
    ) -> Self;

    /// Called after the drawing dimensions have changed (e.g. the surface was
    /// resized).
    fn resize(
        &mut self,
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
    );

    /// Called after text has finished compositing. The provided `text_view` is
    /// the composited text. The final output of your implementation should
    /// render to the provided `surface_view`.
    ///
    /// <div class="warning">
    ///
    /// Retaining a reference to the provided surface view will cause a panic if
    /// the swapchain is recreated.
    ///
    /// </div>
    fn process(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        surface_view: &TextureView,
    );

    /// Called to see if this post processor wants to update the screen. By
    /// default, the backend only runs the compositor and post processor when
    /// the text changes. Returning true from this will override that behavior
    /// and cause the processor to be invoked after a call to flush, even if no
    /// text changes occurred.
    fn needs_update(&self) -> bool {
        false
    }
}

/// Controls the area the text is rendered to relative to the presentation
/// surface.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub enum Viewport {
    /// Render to the entire surface.
    #[default]
    Full,
    /// Render to a reduced area starting at the top right and rendering up to
    /// the bottom left - (width, height).
    Shrink { width: u32, height: u32 },
}

/// Builds a [WgpuBackend] instance.
///
/// Height and width will default to 1x1, so don't forget to call
/// [`Builder::with_dimensions`] to configure the backend presentation
/// dimensions.
pub struct Builder<'a, P: PostProcessor> {
    user_data: P::UserData,
    fonts: Fonts<'a>,
    instance: Option<Instance>,
    limits: Option<Limits>,
    present_mode: Option<PresentMode>,
    width: NonZeroU32,
    height: NonZeroU32,
    viewport: Viewport,
    reset_fg: Srgb<u8>,
    reset_bg: Srgb<u8>,
}

impl<'a, P: PostProcessor> Builder<'a, P>
where
    P::UserData: Default,
{
    /// Create a new Builder from a specified [`Font`] and default
    /// [`PostProcessor::UserData`].
    pub fn from_font(font: Font<'a>) -> Self {
        Self {
            user_data: Default::default(),
            instance: None,
            fonts: Fonts::new(font, 24),
            limits: None,
            present_mode: None,
            width: NonZeroU32::new(1).unwrap(),
            height: NonZeroU32::new(1).unwrap(),
            viewport: Viewport::Full,
            reset_fg: palette::named::BLACK,
            reset_bg: palette::named::WHITE,
        }
    }
}

impl<'a, P: PostProcessor> Builder<'a, P> {
    /// Create a new Builder from a specified [`Font`] and supplied
    /// [`PostProcessor::UserData`].
    pub fn from_font_and_user_data(font: Font<'a>, user_data: P::UserData) -> Self {
        Self {
            user_data,
            instance: None,
            fonts: Fonts::new(font, 24),
            limits: None,
            present_mode: None,
            width: NonZeroU32::new(1).unwrap(),
            height: NonZeroU32::new(1).unwrap(),
            viewport: Viewport::Full,
            reset_fg: palette::named::BLACK,
            reset_bg: palette::named::WHITE,
        }
    }

    /// Use the supplied [`wgpu::Instance`] when building the backend.
    #[must_use]
    pub fn with_instance(mut self, instance: Instance) -> Self {
        self.instance = Some(instance);
        self
    }

    /// Use the supplied [`Viewport`] for rendering. Defaults to
    /// [`Viewport::Full`].
    #[must_use]
    pub fn with_viewport(mut self, viewport: Viewport) -> Self {
        self.viewport = viewport;
        self
    }

    /// Use the specified font size in pixels. Defaults to 24px.
    #[must_use]
    pub fn with_font_size_px(mut self, size: u32) -> Self {
        self.fonts.set_size_px(size);
        self
    }

    /// Use the specified list of regular fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_regular_fonts`].
    #[must_use]
    pub fn with_regular_fonts<I: IntoIterator<Item = Font<'a>>>(mut self, fonts: I) -> Self {
        self.fonts.add_regular_fonts(fonts);
        self
    }

    /// Use the specified list of bold fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_bold_fonts`].
    #[must_use]
    pub fn with_bold_fonts<I: IntoIterator<Item = Font<'a>>>(mut self, fonts: I) -> Self {
        self.fonts.add_bold_fonts(fonts);
        self
    }

    /// Use the specified list of italic fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_italic_fonts`].
    #[must_use]
    pub fn with_italic_fonts<I: IntoIterator<Item = Font<'a>>>(mut self, fonts: I) -> Self {
        self.fonts.add_italic_fonts(fonts);
        self
    }

    /// Use the specified list of bold italic fonts for rendering. You may call
    /// this multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_bold_italic_fonts`].
    #[must_use]
    pub fn with_bold_italic_fonts<I: IntoIterator<Item = Font<'a>>>(mut self, fonts: I) -> Self {
        self.fonts.add_bold_italic_fonts(fonts);
        self
    }

    /// Use the specified [`wgpu::Limits`]. Defaults to
    /// [`wgpu::Adapter::limits`].
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Use the specified [`wgpu::PresentMode`].
    #[must_use]
    pub fn with_present_mode(mut self, mode: PresentMode) -> Self {
        self.present_mode = Some(mode);
        self
    }

    /// Use the specified height and width when creating the surface. Defaults
    /// to 1x1.
    #[must_use]
    pub fn with_dimensions(mut self, height: NonZeroU32, width: NonZeroU32) -> Self {
        self.height = height;
        self.width = width;
        self
    }

    /// Use the specified [`ratatui::style::Color`] for the default foreground
    /// color. Defaults to Black.
    #[must_use]
    pub fn with_fg_color(mut self, fg: Color) -> Self {
        self.reset_fg = c2c(fg, self.reset_fg);
        self
    }

    /// Use the specified [`ratatui::style::Color`] for the default background
    /// color. Defaults to White.
    #[must_use]
    pub fn with_bg_color(mut self, bg: Color) -> Self {
        self.reset_bg = c2c(bg, self.reset_bg);
        self
    }
}

impl<'a, P: PostProcessor> Builder<'a, P> {
    /// Build a new backend with the provided surface target - e.g. a winit
    /// `Window`.
    pub async fn build_with_target<'s>(
        mut self,
        target: impl Into<SurfaceTarget<'s>>,
    ) -> Result<WgpuBackend<'a, 's, P>> {
        let instance = self.instance.get_or_insert_with(|| {
            wgpu::Instance::new(InstanceDescriptor {
                backends: Backends::default(),
                flags: InstanceFlags::default(),
                ..Default::default()
            })
        });
        let surface = instance
            .create_surface(target)
            .map_err(Error::SurfaceCreationFailed)?;

        self.build_with_surface(surface).await
    }

    /// Build a new backend from this builder with the supplied surface. You
    /// almost certainly want to call this with the instance you used to create
    /// the provided surface - see [`Builder::with_instance`]. If one is not
    /// provided, a default instance will be created.
    pub async fn build_with_surface<'s>(
        mut self,
        surface: Surface<'s>,
    ) -> Result<WgpuBackend<'a, 's, P>> {
        let instance = self.instance.get_or_insert_with(|| {
            wgpu::Instance::new(InstanceDescriptor {
                backends: Backends::default(),
                flags: InstanceFlags::default(),
                ..Default::default()
            })
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .ok_or(Error::AdapterRequestFailed)?;

        let limits = if let Some(limits) = self.limits {
            min_limits(&adapter, limits)
        } else {
            adapter.limits()
        };

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_limits: limits.clone(),
                    ..Default::default()
                },
                None,
            )
            .await
            .map_err(Error::DeviceRequestFailed)?;

        let mut surface_config = surface
            .get_default_config(
                &adapter,
                self.width.get().min(limits.max_texture_dimension_2d),
                self.height.get().min(limits.max_texture_dimension_2d),
            )
            .ok_or(Error::SurfaceConfigurationRequestFailed)?;

        if let Some(mode) = self.present_mode {
            surface_config.present_mode = mode;
        }

        surface.configure(&device, &surface_config);

        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let drawable_width = surface_config.width - inset_width;
        let drawable_height = surface_config.height - inset_height;

        info!(
            "char width x height: {}x{}",
            self.fonts.width_px(),
            self.fonts.height_px()
        );

        let text_cache = device.create_texture(&TextureDescriptor {
            label: Some("Text Atlas"),
            size: Extent3d {
                width: CACHE_WIDTH,
                height: CACHE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let text_cache_view = text_cache.create_view(&TextureViewDescriptor::default());

        let text_screen_size_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Uniforms Buffer"),
            size: size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let atlas_size_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Atlas Size buffer"),
            contents: bytemuck::cast_slice(&[CACHE_WIDTH as f32, CACHE_HEIGHT as f32, 0.0, 0.0]),
            usage: BufferUsages::UNIFORM,
        });

        let text_compositor = build_text_compositor(
            &device,
            &text_screen_size_buffer,
            &atlas_size_buffer,
            &text_cache_view,
            &sampler,
        );

        let wgpu_state = build_wgpu_state(
            &device,
            (drawable_width / self.fonts.width_px()) * self.fonts.width_px(),
            (drawable_height / self.fonts.height_px()) * self.fonts.height_px(),
        );

        Ok(WgpuBackend {
            post_process: P::compile(
                &device,
                &wgpu_state.text_dest_view,
                &surface_config,
                self.user_data,
            ),
            cells: vec![],
            dirty_rows: vec![],
            scale_context: ScaleContext::default(),
            cursor: (0, 0),
            surface,
            surface_config,
            device,
            queue,
            shaper: ShapeContext::new(),
            viewport: self.viewport,
            cached: Atlas::new(CACHE_WIDTH, CACHE_HEIGHT),
            text_cache,
            text_indices: vec![],
            text_vertices: vec![],
            text_screen_size_buffer,
            text_compositor,
            wgpu_state,
            fonts: self.fonts,
            reset_fg: self.reset_fg,
            reset_bg: self.reset_bg,
        })
    }
}

/// A ratatui backend leveraging wgpu for rendering.
///
/// Constructed using a [`Builder`].
///
/// Limitations:
/// - The cursor is tracked but not rendered.
/// - No support for blinking text.
/// - No builtin accessibilty, although [`WgpuBackend::get_text`] is provided to
///   access the screen's contents.
pub struct WgpuBackend<'f, 's, P: PostProcessor = DefaultPostProcessor> {
    post_process: P,

    cells: Vec<Cell>,
    dirty_rows: Vec<bool>,
    scale_context: ScaleContext,

    cursor: (u16, u16),

    viewport: Viewport,

    surface: Surface<'s>,
    surface_config: SurfaceConfiguration,
    device: Device,
    queue: Queue,

    shaper: ShapeContext,

    cached: Atlas,
    text_cache: Texture,
    text_indices: Vec<[u16; 6]>,
    text_vertices: Vec<TextVertexMember>,
    text_compositor: TextCachePipeline,
    text_screen_size_buffer: Buffer,

    wgpu_state: WgpuState,

    fonts: Fonts<'f>,
    reset_fg: Srgb<u8>,
    reset_bg: Srgb<u8>,
}

impl<'f, 's, P: PostProcessor> WgpuBackend<'f, 's, P> {
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

        if width == self.surface_config.width && height == self.surface_config.height {
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
        self.surface.configure(&self.device, &self.surface_config);

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
        self.clear().unwrap();
        self.cached.clear();
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

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(err) => {
                error!("{err}");
                return;
            }
        };
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        self.post_process.process(
            &mut encoder,
            &self.queue,
            &self.wgpu_state.text_dest_view,
            &self.surface_config,
            &view,
        );

        self.queue.submit(Some(encoder.finish()));
        output.present();
    }
}

impl<'f, 's, P: PostProcessor> Backend for WgpuBackend<'f, 's, P> {
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
            let mut shape = |font: &Font, fake_bold, fake_italic, cluster: &GlyphCluster| {
                let cell = &row[cluster.data as usize];

                let metrics = font.metrics();
                let advance_scale =
                    self.fonts.height_px() as f32 / (metrics.ascent + metrics.descent);

                let basey = y as u32 * self.fonts.height_px();
                for glyph in cluster.glyphs {
                    let basex = x;
                    let advance = (glyph.advance * advance_scale) as u32;
                    x += advance;

                    let key = Key {
                        style: cell
                            .modifier
                            .intersection(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED),
                        glyph: glyph.id,
                        font: font.key(),
                    };

                    let width = (font
                        .font()
                        .glyph_metrics(&[])
                        .linear_scale(advance_scale)
                        .advance_width(glyph.id) as u32)
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
                        let metrics = font.metrics();
                        let linear_scale =
                            self.fonts.height_px() as f32 / (metrics.ascent + metrics.descent);
                        let metrics = metrics.linear_scale(linear_scale);

                        let ratio = {
                            let scaled = font.metrics().scale(self.fonts.height_px() as f32);
                            self.fonts.height_px() as f32 / (scaled.ascent + scaled.descent)
                        };

                        let mut scaler = self
                            .scale_context
                            .builder(font.font())
                            .hint(true)
                            .size(self.fonts.height_px() as f32 * ratio)
                            .build();

                        let mut render = Render::new(&[
                            Source::Outline,
                            Source::Bitmap(StrikeWith::BestFit),
                            Source::ColorOutline(0),
                            Source::ColorBitmap(StrikeWith::BestFit),
                        ]);

                        if fake_italic {
                            render.transform(Some(
                                Transform::skew(
                                    Angle::from_degrees(14.0),
                                    Angle::from_degrees(0.0),
                                )
                                .then_translate(-(width as f32 * 0.121), 0.0),
                            ));
                        }
                        if fake_bold {
                            render.embolden(0.5);
                        }

                        if let Some(image) = render.render(&mut scaler, glyph.id) {
                            return (cached, Some((metrics, image)));
                        }

                        (cached, None)
                    });
                }
            };

            let script = row
                .iter()
                .flat_map(|r| r.symbol().chars().map(|ch| ch.script()))
                .filter_map(|script| match script {
                    Script::Unknown | Script::Common | Script::Inherited | Script::Latin => None,
                    script => Some(script),
                })
                .next();
            let script = script.unwrap_or(swash::text::Script::Latin);

            let mut parser = Parser::new(
                script,
                row.iter()
                    .enumerate()
                    .flat_map(|(idx, cell)| cell.symbol().chars().map(move |ch| (idx, ch)))
                    .scan(0u32, |state, (idx, ch)| {
                        let offset = *state;
                        *state += ch.len_utf8() as u32;

                        Some(Token {
                            ch,
                            offset,
                            len: ch.len_utf8() as u8,
                            info: ch.into(),
                            data: idx as u32,
                        })
                    }),
            );

            let mut current_font = self.fonts.last_resort();
            let mut shaper = self
                .shaper
                .builder(current_font.font())
                .script(script)
                .build();

            let mut current_fake_bold = false;
            let mut current_fake_italic = false;

            let mut cluster = CharCluster::new();
            while parser.next(&mut cluster) {
                let cell = &row[cluster.user_data() as usize];

                let (font, fake_bold, fake_italic) = self.fonts.font_for_cell(&mut cluster, cell);

                if font.key() != current_font.key()
                    || current_fake_bold != fake_bold
                    || current_fake_italic != fake_italic
                {
                    shaper.shape_with(|cluster| {
                        shape(
                            current_font,
                            current_fake_bold,
                            current_fake_italic,
                            cluster,
                        )
                    });

                    current_font = font;
                    current_fake_bold = fake_bold;
                    current_fake_italic = fake_italic;
                    shaper = self
                        .shaper
                        .builder(current_font.font())
                        .script(script)
                        .build();
                }

                shaper.add_cluster(&cluster);
            }

            shaper.shape_with(|cluster| {
                shape(
                    current_font,
                    current_fake_bold,
                    current_fake_italic,
                    cluster,
                )
            });
        }

        for (key, (cached, maybe_path)) in pending_cache_updates {
            let mut image = vec![0; cached.width as usize * cached.height as usize];

            if let Some((metrics, path)) = maybe_path {
                match path.content {
                    swash::scale::image::Content::Mask => {
                        for (idx, a) in path.data.into_iter().enumerate() {
                            let x = idx as i32 % path.placement.width as i32 + path.placement.left;
                            let y = idx as i32 / path.placement.width as i32
                                + metrics.ascent as i32
                                - path.placement.top;

                            if x < 0
                                || y < 0
                                || x as u32 >= cached.width
                                || y as u32 >= cached.height
                            {
                                continue;
                            }

                            image[y as usize * cached.width as usize + x as usize] = a;
                        }
                    }
                    swash::scale::image::Content::SubpixelMask => {
                        warn!("Subpixel masks are not currently supported")
                    }
                    swash::scale::image::Content::Color => {
                        for (idx, rgba) in path.data.chunks(4).enumerate() {
                            let x = idx as i32 % path.placement.width as i32 + path.placement.left;
                            let y = idx as i32 / path.placement.width as i32
                                + metrics.ascent as i32
                                - path.placement.top;

                            if x < 0
                                || y < 0
                                || x as u32 >= cached.width
                                || y as u32 >= cached.height
                            {
                                continue;
                            }

                            image[y as usize * cached.width as usize + x as usize] = rgba[3];
                        }
                    }
                }

                if key.style.contains(Modifier::UNDERLINED) {
                    let (underline_position, underline_thickness) =
                        (metrics.ascent as u32 + 1, metrics.stroke_size as u32);

                    for y in underline_position..underline_position + underline_thickness {
                        for x in 0..cached.width {
                            image[y as usize * cached.width as usize + x as usize] = 255;
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
                &image,
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

fn build_wgpu_state(device: &Device, drawable_width: u32, drawable_height: u32) -> WgpuState {
    let text_dest = device.create_texture(&TextureDescriptor {
        label: Some("Text Compositor Out"),
        size: Extent3d {
            width: drawable_width,
            height: drawable_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let text_dest_view = text_dest.create_view(&TextureViewDescriptor::default());

    WgpuState { text_dest_view }
}

fn build_text_compositor(
    device: &Device,
    screen_size: &Buffer,
    atlas_size: &Buffer,
    cache_view: &TextureView,
    sampler: &Sampler,
) -> TextCachePipeline {
    let shader = device.create_shader_module(include_wgsl!("shaders/composite.wgsl"));

    let vertex_shader_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Text Compositor Uniforms Binding Layout"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(NonZeroU64::new(size_of::<[f32; 4]>() as u64).unwrap()),
            },
            count: None,
        }],
    });

    let fragment_shader_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Text Compositor Fragment Binding Layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(NonZeroU64::new(size_of::<[f32; 4]>() as u64).unwrap()),
                },
                count: None,
            },
        ],
    });

    let fs_uniforms = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Text Compositor Uniforms Binding"),
        layout: &vertex_shader_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: screen_size.as_entire_binding(),
        }],
    });

    let atlas_bindings = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Text Compositor Fragment Binding"),
        layout: &fragment_shader_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(cache_view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 2,
                resource: atlas_size.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Text Compositor Layout"),
        bind_group_layouts: &[&vertex_shader_layout, &fragment_shader_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("Text Compositor Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: "vs_main",
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[VertexBufferLayout {
                array_stride: size_of::<TextVertexMember>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32, 3 => Uint32],
            }],
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: "fs_main",
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });

    TextCachePipeline {
        pipeline,
        fs_uniforms,
        atlas_bindings,
    }
}

fn min_limits(adapter: &wgpu::Adapter, limits: Limits) -> Limits {
    let Limits {
        max_texture_dimension_1d: max_texture_dimension_1d_wl,
        max_texture_dimension_2d: max_texture_dimension_2d_wl,
        max_texture_dimension_3d: max_texture_dimension_3d_wl,
        max_texture_array_layers: max_texture_array_layers_wl,
        max_bind_groups: max_bind_groups_wl,
        max_bindings_per_bind_group: max_bindings_per_bind_group_wl,
        max_dynamic_uniform_buffers_per_pipeline_layout:
            max_dynamic_uniform_buffers_per_pipeline_layout_wl,
        max_dynamic_storage_buffers_per_pipeline_layout:
            max_dynamic_storage_buffers_per_pipeline_layout_wl,
        max_sampled_textures_per_shader_stage: max_sampled_textures_per_shader_stage_wl,
        max_samplers_per_shader_stage: max_samplers_per_shader_stage_wl,
        max_storage_buffers_per_shader_stage: max_storage_buffers_per_shader_stage_wl,
        max_storage_textures_per_shader_stage: max_storage_textures_per_shader_stage_wl,
        max_uniform_buffers_per_shader_stage: max_uniform_buffers_per_shader_stage_wl,
        max_uniform_buffer_binding_size: max_uniform_buffer_binding_size_wl,
        max_storage_buffer_binding_size: max_storage_buffer_binding_size_wl,
        max_vertex_buffers: max_vertex_buffers_wl,
        max_buffer_size: max_buffer_size_wl,
        max_vertex_attributes: max_vertex_attributes_wl,
        max_vertex_buffer_array_stride: max_vertex_buffer_array_stride_wl,
        min_uniform_buffer_offset_alignment: min_uniform_buffer_offset_alignment_wl,
        min_storage_buffer_offset_alignment: min_storage_buffer_offset_alignment_wl,
        max_inter_stage_shader_components: max_inter_stage_shader_components_wl,
        max_color_attachments: max_color_attachments_wl,
        max_color_attachment_bytes_per_sample: max_color_attachment_bytes_per_sample_wl,
        max_compute_workgroup_storage_size: max_compute_workgroup_storage_size_wl,
        max_compute_invocations_per_workgroup: max_compute_invocations_per_workgroup_wl,
        max_compute_workgroup_size_x: max_compute_workgroup_size_x_wl,
        max_compute_workgroup_size_y: max_compute_workgroup_size_y_wl,
        max_compute_workgroup_size_z: max_compute_workgroup_size_z_wl,
        max_compute_workgroups_per_dimension: max_compute_workgroups_per_dimension_wl,
        min_subgroup_size: min_subgroup_size_wl,
        max_subgroup_size: max_subgroup_size_wl,
        max_push_constant_size: max_push_constant_size_wl,
        max_non_sampler_bindings: max_non_sampler_bindings_wl,
    } = limits;
    let Limits {
        max_texture_dimension_1d: max_texture_dimension_1d_al,
        max_texture_dimension_2d: max_texture_dimension_2d_al,
        max_texture_dimension_3d: max_texture_dimension_3d_al,
        max_texture_array_layers: max_texture_array_layers_al,
        max_bind_groups: max_bind_groups_al,
        max_bindings_per_bind_group: max_bindings_per_bind_group_al,
        max_dynamic_uniform_buffers_per_pipeline_layout:
            max_dynamic_uniform_buffers_per_pipeline_layout_al,
        max_dynamic_storage_buffers_per_pipeline_layout:
            max_dynamic_storage_buffers_per_pipeline_layout_al,
        max_sampled_textures_per_shader_stage: max_sampled_textures_per_shader_stage_al,
        max_samplers_per_shader_stage: max_samplers_per_shader_stage_al,
        max_storage_buffers_per_shader_stage: max_storage_buffers_per_shader_stage_al,
        max_storage_textures_per_shader_stage: max_storage_textures_per_shader_stage_al,
        max_uniform_buffers_per_shader_stage: max_uniform_buffers_per_shader_stage_al,
        max_uniform_buffer_binding_size: max_uniform_buffer_binding_size_al,
        max_storage_buffer_binding_size: max_storage_buffer_binding_size_al,
        max_vertex_buffers: max_vertex_buffers_al,
        max_buffer_size: max_buffer_size_al,
        max_vertex_attributes: max_vertex_attributes_al,
        max_vertex_buffer_array_stride: max_vertex_buffer_array_stride_al,
        min_uniform_buffer_offset_alignment: min_uniform_buffer_offset_alignment_al,
        min_storage_buffer_offset_alignment: min_storage_buffer_offset_alignment_al,
        max_inter_stage_shader_components: max_inter_stage_shader_components_al,
        max_color_attachments: max_color_attachments_al,
        max_color_attachment_bytes_per_sample: max_color_attachment_bytes_per_sample_al,
        max_compute_workgroup_storage_size: max_compute_workgroup_storage_size_al,
        max_compute_invocations_per_workgroup: max_compute_invocations_per_workgroup_al,
        max_compute_workgroup_size_x: max_compute_workgroup_size_x_al,
        max_compute_workgroup_size_y: max_compute_workgroup_size_y_al,
        max_compute_workgroup_size_z: max_compute_workgroup_size_z_al,
        max_compute_workgroups_per_dimension: max_compute_workgroups_per_dimension_al,
        min_subgroup_size: min_subgroup_size_al,
        max_subgroup_size: max_subgroup_size_al,
        max_push_constant_size: max_push_constant_size_al,
        max_non_sampler_bindings: max_non_sampler_bindings_al,
    } = adapter.limits();

    Limits {
        max_texture_dimension_1d: if max_texture_dimension_1d_wl <= max_texture_dimension_1d_al {
            max_texture_dimension_1d_wl
        } else {
            max_texture_dimension_1d_al
        },
        max_texture_dimension_2d: if max_texture_dimension_2d_wl <= max_texture_dimension_2d_al {
            max_texture_dimension_2d_wl
        } else {
            max_texture_dimension_2d_al
        },
        max_texture_dimension_3d: if max_texture_dimension_3d_wl <= max_texture_dimension_3d_al {
            max_texture_dimension_3d_wl
        } else {
            max_texture_dimension_3d_al
        },
        max_texture_array_layers: if max_texture_array_layers_wl <= max_texture_array_layers_al {
            max_texture_array_layers_wl
        } else {
            max_texture_array_layers_al
        },
        max_bind_groups: if max_bind_groups_wl <= max_bind_groups_al {
            max_bind_groups_wl
        } else {
            max_bind_groups_al
        },
        max_bindings_per_bind_group: if max_bindings_per_bind_group_wl
            <= max_bindings_per_bind_group_al
        {
            max_bindings_per_bind_group_wl
        } else {
            max_bindings_per_bind_group_al
        },
        max_dynamic_uniform_buffers_per_pipeline_layout:
            if max_dynamic_uniform_buffers_per_pipeline_layout_wl
                <= max_dynamic_uniform_buffers_per_pipeline_layout_al
            {
                max_dynamic_uniform_buffers_per_pipeline_layout_wl
            } else {
                max_dynamic_uniform_buffers_per_pipeline_layout_al
            },
        max_dynamic_storage_buffers_per_pipeline_layout:
            if max_dynamic_storage_buffers_per_pipeline_layout_wl
                <= max_dynamic_storage_buffers_per_pipeline_layout_al
            {
                max_dynamic_storage_buffers_per_pipeline_layout_wl
            } else {
                max_dynamic_storage_buffers_per_pipeline_layout_al
            },
        max_sampled_textures_per_shader_stage: if max_sampled_textures_per_shader_stage_wl
            <= max_sampled_textures_per_shader_stage_al
        {
            max_sampled_textures_per_shader_stage_wl
        } else {
            max_sampled_textures_per_shader_stage_al
        },
        max_samplers_per_shader_stage: if max_samplers_per_shader_stage_wl
            <= max_samplers_per_shader_stage_al
        {
            max_samplers_per_shader_stage_wl
        } else {
            max_samplers_per_shader_stage_al
        },
        max_storage_buffers_per_shader_stage: if max_storage_buffers_per_shader_stage_wl
            <= max_storage_buffers_per_shader_stage_al
        {
            max_storage_buffers_per_shader_stage_wl
        } else {
            max_storage_buffers_per_shader_stage_al
        },
        max_storage_textures_per_shader_stage: if max_storage_textures_per_shader_stage_wl
            <= max_storage_textures_per_shader_stage_al
        {
            max_storage_textures_per_shader_stage_wl
        } else {
            max_storage_textures_per_shader_stage_al
        },
        max_uniform_buffers_per_shader_stage: if max_uniform_buffers_per_shader_stage_wl
            <= max_uniform_buffers_per_shader_stage_al
        {
            max_uniform_buffers_per_shader_stage_wl
        } else {
            max_uniform_buffers_per_shader_stage_al
        },
        max_uniform_buffer_binding_size: if max_uniform_buffer_binding_size_wl
            <= max_uniform_buffer_binding_size_al
        {
            max_uniform_buffer_binding_size_wl
        } else {
            max_uniform_buffer_binding_size_al
        },
        max_storage_buffer_binding_size: if max_storage_buffer_binding_size_wl
            <= max_storage_buffer_binding_size_al
        {
            max_storage_buffer_binding_size_wl
        } else {
            max_storage_buffer_binding_size_al
        },
        max_vertex_buffers: if max_vertex_buffers_wl <= max_vertex_buffers_al {
            max_vertex_buffers_wl
        } else {
            max_vertex_buffers_al
        },
        max_buffer_size: if max_buffer_size_wl <= max_buffer_size_al {
            max_buffer_size_wl
        } else {
            max_buffer_size_al
        },
        max_vertex_attributes: if max_vertex_attributes_wl <= max_vertex_attributes_al {
            max_vertex_attributes_wl
        } else {
            max_vertex_attributes_al
        },
        max_vertex_buffer_array_stride: if max_vertex_buffer_array_stride_wl
            <= max_vertex_buffer_array_stride_al
        {
            max_vertex_buffer_array_stride_wl
        } else {
            max_vertex_buffer_array_stride_al
        },
        min_uniform_buffer_offset_alignment: if min_uniform_buffer_offset_alignment_wl
            <= min_uniform_buffer_offset_alignment_al
        {
            min_uniform_buffer_offset_alignment_wl
        } else {
            min_uniform_buffer_offset_alignment_al
        },
        min_storage_buffer_offset_alignment: if min_storage_buffer_offset_alignment_wl
            <= min_storage_buffer_offset_alignment_al
        {
            min_storage_buffer_offset_alignment_wl
        } else {
            min_storage_buffer_offset_alignment_al
        },
        max_inter_stage_shader_components: if max_inter_stage_shader_components_wl
            <= max_inter_stage_shader_components_al
        {
            max_inter_stage_shader_components_wl
        } else {
            max_inter_stage_shader_components_al
        },
        max_color_attachments: if max_color_attachments_wl <= max_color_attachments_al {
            max_color_attachments_wl
        } else {
            max_color_attachments_al
        },
        max_color_attachment_bytes_per_sample: if max_color_attachment_bytes_per_sample_wl
            <= max_color_attachment_bytes_per_sample_al
        {
            max_color_attachment_bytes_per_sample_wl
        } else {
            max_color_attachment_bytes_per_sample_al
        },
        max_compute_workgroup_storage_size: if max_compute_workgroup_storage_size_wl
            <= max_compute_workgroup_storage_size_al
        {
            max_compute_workgroup_storage_size_wl
        } else {
            max_compute_workgroup_storage_size_al
        },
        max_compute_invocations_per_workgroup: if max_compute_invocations_per_workgroup_wl
            <= max_compute_invocations_per_workgroup_al
        {
            max_compute_invocations_per_workgroup_wl
        } else {
            max_compute_invocations_per_workgroup_al
        },
        max_compute_workgroup_size_x: if max_compute_workgroup_size_x_wl
            <= max_compute_workgroup_size_x_al
        {
            max_compute_workgroup_size_x_wl
        } else {
            max_compute_workgroup_size_x_al
        },
        max_compute_workgroup_size_y: if max_compute_workgroup_size_y_wl
            <= max_compute_workgroup_size_y_al
        {
            max_compute_workgroup_size_y_wl
        } else {
            max_compute_workgroup_size_y_al
        },
        max_compute_workgroup_size_z: if max_compute_workgroup_size_z_wl
            <= max_compute_workgroup_size_z_al
        {
            max_compute_workgroup_size_z_wl
        } else {
            max_compute_workgroup_size_z_al
        },
        max_compute_workgroups_per_dimension: if max_compute_workgroups_per_dimension_wl
            <= max_compute_workgroups_per_dimension_al
        {
            max_compute_workgroups_per_dimension_wl
        } else {
            max_compute_workgroups_per_dimension_al
        },
        min_subgroup_size: if min_subgroup_size_wl <= min_subgroup_size_al {
            min_subgroup_size_wl
        } else {
            min_subgroup_size_al
        },
        max_subgroup_size: if max_subgroup_size_wl <= max_subgroup_size_al {
            max_subgroup_size_wl
        } else {
            max_subgroup_size_al
        },
        max_push_constant_size: if max_push_constant_size_wl <= max_push_constant_size_al {
            max_push_constant_size_wl
        } else {
            max_push_constant_size_al
        },
        max_non_sampler_bindings: if max_non_sampler_bindings_wl <= max_non_sampler_bindings_al {
            max_non_sampler_bindings_wl
        } else {
            max_non_sampler_bindings_al
        },
    }
}
