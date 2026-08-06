use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::num::NonZeroU64;

use bitvec::vec::BitVec;
use ratatui_core::style::Color;
use rustybuzz::UnicodeBuffer;
use web_time::Duration;
use web_time::Instant;
use wgpu::include_wgsl;
use wgpu::util::BufferInitDescriptor;
use wgpu::util::DeviceExt;
use wgpu::vertex_attr_array;
use wgpu::AddressMode;
use wgpu::Backends;
use wgpu::BindGroupDescriptor;
use wgpu::BindGroupEntry;
use wgpu::BindGroupLayoutDescriptor;
use wgpu::BindGroupLayoutEntry;
use wgpu::BindingResource;
use wgpu::BindingType;
use wgpu::BlendState;
use wgpu::Buffer;
use wgpu::BufferBindingType;
use wgpu::BufferDescriptor;
use wgpu::BufferUsages;
use wgpu::ColorTargetState;
use wgpu::ColorWrites;
use wgpu::Device;
use wgpu::Extent3d;
use wgpu::FilterMode;
use wgpu::FragmentState;
use wgpu::Instance;
use wgpu::InstanceDescriptor;
use wgpu::InstanceFlags;
use wgpu::Limits;
use wgpu::MipmapFilterMode;
use wgpu::MultisampleState;
use wgpu::PipelineCompilationOptions;
use wgpu::PipelineLayoutDescriptor;
use wgpu::PresentMode;
use wgpu::PrimitiveState;
use wgpu::PrimitiveTopology;
use wgpu::RenderPipelineDescriptor;
use wgpu::Sampler;
use wgpu::SamplerBindingType;
use wgpu::SamplerDescriptor;
use wgpu::ShaderStages;
use wgpu::Surface;
use wgpu::SurfaceTarget;
use wgpu::TextureDescriptor;
use wgpu::TextureDimension;
use wgpu::TextureFormat;
use wgpu::TextureSampleType;
use wgpu::TextureUsages;
use wgpu::TextureView;
use wgpu::TextureViewDescriptor;
use wgpu::TextureViewDimension;
use wgpu::VertexBufferLayout;
use wgpu::VertexState;
use wgpu::VertexStepMode;

use crate::backend::build_wgpu_state;
use crate::backend::private::Token;
use crate::backend::wgpu_backend::WgpuBackend;
use crate::backend::Dimensions;
use crate::backend::PostProcessor;
use crate::backend::RenderSurface;
use crate::backend::TextBgVertexMember;
use crate::backend::TextCacheBgPipeline;
use crate::backend::TextCacheFgPipeline;
use crate::backend::TextVertexMember;
use crate::backend::Viewport;
use crate::colors::named;
use crate::colors::ColorTable;
use crate::fonts::Font;
use crate::fonts::Fonts;
use crate::shaders::DefaultPostProcessor;
use crate::utils::plan_cache::PlanCache;
use crate::utils::text_atlas::Atlas;
use crate::Error;
use crate::Result;

const CACHE_WIDTH: u32 = 1800;
const CACHE_HEIGHT: u32 = 1200;

/// Builds a [`WgpuBackend`] instance.
///
/// Height and width will default to 1x1, so don't forget to call
/// [`Builder::with_dimensions`] to configure the backend presentation
/// dimensions.
pub struct Builder<'a, P: PostProcessor = DefaultPostProcessor> {
    user_data: P::UserData,
    fonts: Fonts<'a>,
    instance: Option<Instance>,
    limits: Option<Limits>,
    present_mode: Option<PresentMode>,
    width: NonZeroU32,
    height: NonZeroU32,
    viewport: Viewport,
    colors: ColorTable,
    reset_fg: Color,
    reset_bg: Color,
    fast_blink: Duration,
    slow_blink: Duration,
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
            colors: named::DEFAULT_COLORS,
            reset_fg: Color::Black,
            reset_bg: Color::White,
            fast_blink: Duration::from_millis(200),
            slow_blink: Duration::from_millis(1000),
        }
    }
}

impl<'a, P: PostProcessor> Builder<'a, P> {
    /// Create a new Builder from a specified [`Font`] and supplied
    /// [`PostProcessor::UserData`].
    pub fn from_font_and_user_data(
        font: Font<'a>,
        user_data: P::UserData,
    ) -> Self {
        Self {
            user_data,
            instance: None,
            fonts: Fonts::new(font, 24),
            limits: None,
            present_mode: None,
            width: NonZeroU32::new(1).unwrap(),
            height: NonZeroU32::new(1).unwrap(),
            viewport: Viewport::Full,
            colors: named::DEFAULT_COLORS,
            reset_fg: Color::Black,
            reset_bg: Color::White,
            fast_blink: Duration::from_millis(200),
            slow_blink: Duration::from_millis(1000),
        }
    }

    /// Use the supplied [`wgpu::Instance`] when building the backend.
    #[must_use]
    pub fn with_instance(
        mut self,
        instance: Instance,
    ) -> Self {
        self.instance = Some(instance);
        self
    }

    /// Use the supplied [`Viewport`] for rendering. Defaults to
    /// [`Viewport::Full`].
    #[must_use]
    pub fn with_viewport(
        mut self,
        viewport: Viewport,
    ) -> Self {
        self.viewport = viewport;
        self
    }

    /// Use the specified font size in pixels. Defaults to 24px.
    #[must_use]
    pub fn with_font_size_px(
        mut self,
        size: u32,
    ) -> Self {
        self.fonts.set_size_px(size);
        self
    }

    /// Use the specified list of fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts. Note that this will
    /// automatically organize fonts by relative width in order to optimize
    /// fallback rendering quality. The ordering of already provided fonts will
    /// remain unchanged.
    ///
    /// See also [`Fonts::add_fonts`].
    pub fn with_fonts<I: IntoIterator<Item = Font<'a>>>(
        mut self,
        fonts: I,
    ) -> Self {
        self.fonts.add_fonts(fonts);
        self
    }

    /// Use the specified list of regular fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_regular_fonts`].
    #[must_use]
    pub fn with_regular_fonts<I: IntoIterator<Item = Font<'a>>>(
        mut self,
        fonts: I,
    ) -> Self {
        self.fonts.add_regular_fonts(fonts);
        self
    }

    /// Use the specified list of bold fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_bold_fonts`].
    #[must_use]
    pub fn with_bold_fonts<I: IntoIterator<Item = Font<'a>>>(
        mut self,
        fonts: I,
    ) -> Self {
        self.fonts.add_bold_fonts(fonts);
        self
    }

    /// Use the specified list of italic fonts for rendering. You may call this
    /// multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_italic_fonts`].
    #[must_use]
    pub fn with_italic_fonts<I: IntoIterator<Item = Font<'a>>>(
        mut self,
        fonts: I,
    ) -> Self {
        self.fonts.add_italic_fonts(fonts);
        self
    }

    /// Use the specified list of bold italic fonts for rendering. You may call
    /// this multiple times to extend the list of fallback fonts.
    ///
    /// See also [`Fonts::add_bold_italic_fonts`].
    #[must_use]
    pub fn with_bold_italic_fonts<I: IntoIterator<Item = Font<'a>>>(
        mut self,
        fonts: I,
    ) -> Self {
        self.fonts.add_bold_italic_fonts(fonts);
        self
    }

    /// Use the specified [`wgpu::Limits`]. Defaults to
    /// [`wgpu::Adapter::limits`].
    #[must_use]
    pub fn with_limits(
        mut self,
        limits: Limits,
    ) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Use the specified [`wgpu::PresentMode`].
    #[must_use]
    pub fn with_present_mode(
        mut self,
        mode: PresentMode,
    ) -> Self {
        self.present_mode = Some(mode);
        self
    }

    /// Use the specified height and width when creating the surface. Defaults
    /// to 1x1.
    #[must_use]
    pub fn with_dimensions(
        mut self,
        dimensions: Dimensions,
    ) -> Self {
        self.width = dimensions.width;
        self.height = dimensions.height;
        self
    }

    /// Use the specified height and width when creating the surface. Defaults
    /// to 1x1.
    #[must_use]
    pub fn with_width_and_height(
        mut self,
        dimensions: Dimensions,
    ) -> Self {
        self.width = dimensions.width;
        self.height = dimensions.height;
        self
    }

    /// Use the specified [`ColorTable`] for the base-16 colors.
    /// There is a default value for this.
    pub fn with_color_table(
        mut self,
        colors: ColorTable,
    ) -> Self {
        self.colors = colors;
        self
    }

    /// Use the specified [`ratatui::style::Color`] for the default foreground
    /// color. Defaults to Black.
    #[must_use]
    pub fn with_fg_color(
        mut self,
        fg: Color,
    ) -> Self {
        self.reset_fg = fg;
        self
    }

    /// Use the specified [`ratatui::style::Color`] for the default background
    /// color. Defaults to White.
    #[must_use]
    pub fn with_bg_color(
        mut self,
        bg: Color,
    ) -> Self {
        self.reset_bg = bg;
        self
    }

    /// Use the specified interval in milliseconds as the rapid blink speed.
    /// Note that this library doesn't spin off rendering into a separate thread
    /// for you. If you want text to blink, you must ensure that a call to
    /// `flush` is made frequently enough. Defaults to 200ms.
    #[must_use]
    pub fn with_rapid_blink_millis(
        mut self,
        millis: u64,
    ) -> Self {
        self.fast_blink = Duration::from_millis(millis);
        self
    }

    /// Use the specified interval in milliseconds as the slow blink speed.
    /// Note that this library doesn't spin off rendering into a separate thread
    /// for you. If you want text to blink, you must ensure that a call to
    /// `flush` is made frequently enough. Defaults to 1000ms.
    #[must_use]
    pub fn with_slow_blink_millis(
        mut self,
        millis: u64,
    ) -> Self {
        self.slow_blink = Duration::from_millis(millis);
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
                ..InstanceDescriptor::new_without_display_handle()
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
        self,
        surface: Surface<'s>,
    ) -> Result<WgpuBackend<'a, 's, P>> {
        self.build_with_render_surface(surface).await
    }

    #[cfg(test)]
    pub(crate) async fn build_headless(
        self
    ) -> Result<WgpuBackend<'a, 'static, P, super::HeadlessSurface>> {
        self.build_with_render_surface(super::HeadlessSurface::default())
            .await
    }

    #[cfg(test)]
    pub(crate) async fn build_headless_with_format(
        self,
        format: TextureFormat,
    ) -> Result<WgpuBackend<'a, 'static, P, super::HeadlessSurface>> {
        self.build_with_render_surface(super::HeadlessSurface::new(format))
            .await
    }

    async fn build_with_render_surface<'s, S: RenderSurface<'s> + 's>(
        mut self,
        mut surface: S,
    ) -> Result<WgpuBackend<'a, 's, P, S>> {
        let instance = self.instance.get_or_insert_with(|| {
            wgpu::Instance::new(InstanceDescriptor {
                backends: Backends::default(),
                flags: InstanceFlags::default(),
                ..InstanceDescriptor::new_without_display_handle()
            })
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface.wgpu_surface(Token),
                ..Default::default()
            })
            .await
            .map_err(Error::AdapterRequestFailed)?;

        let limits = if let Some(limits) = self.limits {
            min_limits(&adapter, limits)
        } else {
            adapter.limits()
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: limits.clone(),
                ..Default::default()
            })
            .await
            .map_err(Error::DeviceRequestFailed)?;

        let mut surface_config = surface
            .get_default_config(
                &adapter,
                self.width.get().min(limits.max_texture_dimension_2d),
                self.height.get().min(limits.max_texture_dimension_2d),
                Token,
            )
            .ok_or(Error::SurfaceConfigurationRequestFailed)?;

        if let Some(mode) = self.present_mode {
            surface_config.present_mode = mode;
        }

        surface.configure(&device, &surface_config, Token);

        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let drawable_width = surface_config.width - inset_width;
        let drawable_height = surface_config.height - inset_height;

        info!(
            "char width x height: {}x{}",
            self.fonts.min_width_px(),
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
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let text_cache_view = text_cache.create_view(&TextureViewDescriptor::default());

        let text_mask = device.create_texture(&TextureDescriptor {
            label: Some("Text Mask"),
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

        let text_mask_view = text_mask.create_view(&TextureViewDescriptor::default());

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

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

        let text_bg_compositor = build_text_bg_compositor(&device, &text_screen_size_buffer);

        let text_fg_compositor = build_text_fg_compositor(
            &device,
            &text_screen_size_buffer,
            &atlas_size_buffer,
            &text_cache_view,
            &text_mask_view,
            &sampler,
        );

        let wgpu_state = build_wgpu_state(
            &device,
            (drawable_width / self.fonts.min_width_px()) * self.fonts.min_width_px(),
            (drawable_height / self.fonts.height_px()) * self.fonts.height_px(),
        );

        let reset_fg = self.colors.c2c(self.reset_fg, [0, 0, 0]);
        let reset_bg = self.colors.c2c(self.reset_bg, [255, 255, 255]);

        Ok(WgpuBackend {
            post_process: P::compile(
                &device,
                &wgpu_state.text_dest_view,
                &surface_config,
                self.user_data,
            ),
            cells: vec![],
            dirty_rows: vec![],
            dirty_cells: BitVec::new(),
            rendered: vec![],
            sourced: vec![],
            fast_blinking: BitVec::new(),
            slow_blinking: BitVec::new(),
            cursor: (0, 0),
            surface,
            _surface: PhantomData,
            surface_config,
            device,
            queue,
            plan_cache: PlanCache::new(self.fonts.count().max(2)),
            buffer: UnicodeBuffer::new(),
            row: String::new(),
            rowmap: vec![],
            viewport: self.viewport,
            cached: Atlas::new(&self.fonts, CACHE_WIDTH, CACHE_HEIGHT),
            text_cache,
            text_mask,
            bg_vertices: vec![],
            text_indices: vec![],
            text_vertices: vec![],
            text_screen_size_buffer,
            text_bg_compositor,
            text_fg_compositor,
            wgpu_state,
            fonts: self.fonts,
            colors: self.colors,
            reset_fg,
            reset_bg,
            fast_duration: self.fast_blink,
            last_fast_toggle: Instant::now(),
            show_fast: true,
            slow_duration: self.slow_blink,
            last_slow_toggle: Instant::now(),
            show_slow: true,
        })
    }
}

fn build_text_bg_compositor(
    device: &Device,
    screen_size: &Buffer,
) -> TextCacheBgPipeline {
    let shader = device.create_shader_module(include_wgsl!("shaders/composite_bg.wgsl"));

    let vertex_shader_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Text Bg Compositor Uniforms Binding Layout"),
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

    let fs_uniforms = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Text Bg Compositor Uniforms Binding"),
        layout: &vertex_shader_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: screen_size.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Text Bg Compositor Layout"),
        bind_group_layouts: &[Some(&vertex_shader_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("Text Bg Compositor Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[Some(VertexBufferLayout {
                array_stride: size_of::<TextBgVertexMember>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertex_attr_array![0 => Float32x2, 1 => Uint32],
            })],
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    TextCacheBgPipeline {
        pipeline,
        fs_uniforms,
    }
}

fn build_text_fg_compositor(
    device: &Device,
    screen_size: &Buffer,
    atlas_size: &Buffer,
    cache_view: &TextureView,
    mask_view: &TextureView,
    sampler: &Sampler,
) -> TextCacheFgPipeline {
    let shader = device.create_shader_module(include_wgsl!("shaders/composite_fg.wgsl"));

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
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
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
                resource: BindingResource::TextureView(mask_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: atlas_size.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Text Compositor Layout"),
        bind_group_layouts: &[Some(&vertex_shader_layout), Some(&fragment_shader_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("Text Compositor Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[Some(VertexBufferLayout {
                array_stride: size_of::<TextVertexMember>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32, 3 => Uint32, 4 => Uint32, 5 => Uint32, 6 => Uint32],
            })],
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    TextCacheFgPipeline {
        pipeline,
        fs_uniforms,
        atlas_bindings,
    }
}

fn min_limits(
    adapter: &wgpu::Adapter,
    wanted: Limits,
) -> Limits {
    let available = adapter.limits();

    Limits {
        max_texture_dimension_1d: wanted
            .max_texture_dimension_1d
            .min(available.max_texture_dimension_1d),
        max_texture_dimension_2d: wanted
            .max_texture_dimension_2d
            .min(available.max_texture_dimension_2d),
        max_texture_dimension_3d: wanted
            .max_texture_dimension_3d
            .min(available.max_texture_dimension_3d),
        max_texture_array_layers: wanted
            .max_texture_array_layers
            .min(available.max_texture_array_layers),
        max_bind_groups: wanted.max_bind_groups.min(available.max_bind_groups),
        max_bind_groups_plus_vertex_buffers: wanted
            .max_bind_groups_plus_vertex_buffers
            .min(available.max_bind_groups_plus_vertex_buffers),
        max_bindings_per_bind_group: wanted
            .max_bindings_per_bind_group
            .min(available.max_bindings_per_bind_group),
        max_dynamic_uniform_buffers_per_pipeline_layout: wanted
            .max_dynamic_uniform_buffers_per_pipeline_layout
            .min(available.max_dynamic_uniform_buffers_per_pipeline_layout),
        max_dynamic_storage_buffers_per_pipeline_layout: wanted
            .max_dynamic_storage_buffers_per_pipeline_layout
            .min(available.max_dynamic_storage_buffers_per_pipeline_layout),
        max_sampled_textures_per_shader_stage: wanted
            .max_sampled_textures_per_shader_stage
            .min(available.max_sampled_textures_per_shader_stage),
        max_samplers_per_shader_stage: wanted
            .max_samplers_per_shader_stage
            .min(available.max_samplers_per_shader_stage),
        max_storage_buffers_per_shader_stage: wanted
            .max_storage_buffers_per_shader_stage
            .min(available.max_storage_buffers_per_shader_stage),
        max_storage_textures_per_shader_stage: wanted
            .max_storage_textures_per_shader_stage
            .min(available.max_storage_textures_per_shader_stage),
        max_uniform_buffers_per_shader_stage: wanted
            .max_uniform_buffers_per_shader_stage
            .min(available.max_uniform_buffers_per_shader_stage),
        max_binding_array_elements_per_shader_stage: wanted
            .max_binding_array_elements_per_shader_stage
            .min(available.max_binding_array_elements_per_shader_stage),
        max_binding_array_acceleration_structure_elements_per_shader_stage: wanted
            .max_binding_array_acceleration_structure_elements_per_shader_stage
            .min(available.max_binding_array_acceleration_structure_elements_per_shader_stage),
        max_binding_array_sampler_elements_per_shader_stage: wanted
            .max_binding_array_sampler_elements_per_shader_stage
            .min(available.max_binding_array_sampler_elements_per_shader_stage),
        max_uniform_buffer_binding_size: wanted
            .max_uniform_buffer_binding_size
            .min(available.max_uniform_buffer_binding_size),
        max_storage_buffer_binding_size: wanted
            .max_storage_buffer_binding_size
            .min(available.max_storage_buffer_binding_size),
        max_vertex_buffers: wanted.max_vertex_buffers.min(available.max_vertex_buffers),
        max_buffer_size: wanted.max_buffer_size.min(available.max_buffer_size),
        max_vertex_attributes: wanted
            .max_vertex_attributes
            .min(available.max_vertex_attributes),
        max_vertex_buffer_array_stride: wanted
            .max_vertex_buffer_array_stride
            .min(available.max_vertex_buffer_array_stride),
        max_inter_stage_shader_variables: wanted
            .max_inter_stage_shader_variables
            .min(available.max_inter_stage_shader_variables),
        min_uniform_buffer_offset_alignment: wanted
            .min_uniform_buffer_offset_alignment
            .min(available.min_uniform_buffer_offset_alignment),
        min_storage_buffer_offset_alignment: wanted
            .min_storage_buffer_offset_alignment
            .min(available.min_storage_buffer_offset_alignment),
        max_color_attachments: wanted
            .max_color_attachments
            .min(available.max_color_attachments),
        max_color_attachment_bytes_per_sample: wanted
            .max_color_attachment_bytes_per_sample
            .min(available.max_color_attachment_bytes_per_sample),
        max_compute_workgroup_storage_size: wanted
            .max_compute_workgroup_storage_size
            .min(available.max_compute_workgroup_storage_size),
        max_compute_invocations_per_workgroup: wanted
            .max_compute_invocations_per_workgroup
            .min(available.max_compute_invocations_per_workgroup),
        max_compute_workgroup_size_x: wanted
            .max_compute_workgroup_size_x
            .min(available.max_compute_workgroup_size_x),
        max_compute_workgroup_size_y: wanted
            .max_compute_workgroup_size_y
            .min(available.max_compute_workgroup_size_y),
        max_compute_workgroup_size_z: wanted
            .max_compute_workgroup_size_z
            .min(available.max_compute_workgroup_size_z),
        max_compute_workgroups_per_dimension: wanted
            .max_compute_workgroups_per_dimension
            .min(available.max_compute_workgroups_per_dimension),
        max_immediate_size: wanted.max_immediate_size.min(available.max_immediate_size),
        max_non_sampler_bindings: wanted
            .max_non_sampler_bindings
            .min(available.max_non_sampler_bindings),
        max_task_workgroup_total_count: wanted
            .max_task_workgroup_total_count
            .min(available.max_task_workgroup_total_count),
        max_task_workgroups_per_dimension: wanted
            .max_task_workgroups_per_dimension
            .min(available.max_task_workgroups_per_dimension),
        max_mesh_workgroup_total_count: wanted
            .max_mesh_workgroup_total_count
            .min(available.max_mesh_workgroup_total_count),
        max_mesh_workgroups_per_dimension: wanted
            .max_mesh_workgroups_per_dimension
            .min(available.max_mesh_workgroups_per_dimension),
        max_task_invocations_per_workgroup: wanted
            .max_task_invocations_per_workgroup
            .min(available.max_task_invocations_per_workgroup),
        max_task_invocations_per_dimension: wanted
            .max_task_invocations_per_dimension
            .min(available.max_task_invocations_per_dimension),
        max_mesh_invocations_per_workgroup: wanted
            .max_mesh_invocations_per_workgroup
            .min(available.max_mesh_invocations_per_workgroup),
        max_mesh_invocations_per_dimension: wanted
            .max_mesh_invocations_per_dimension
            .min(available.max_mesh_invocations_per_dimension),
        max_task_payload_size: wanted
            .max_task_payload_size
            .min(available.max_task_payload_size),
        max_mesh_output_vertices: wanted
            .max_mesh_output_vertices
            .min(available.max_mesh_output_vertices),
        max_mesh_output_primitives: wanted
            .max_mesh_output_primitives
            .min(available.max_mesh_output_primitives),
        max_mesh_output_layers: wanted
            .max_mesh_output_layers
            .min(available.max_mesh_output_layers),
        max_mesh_multiview_view_count: wanted
            .max_mesh_multiview_view_count
            .min(available.max_mesh_multiview_view_count),
        max_blas_primitive_count: wanted
            .max_blas_primitive_count
            .min(available.max_blas_primitive_count),
        max_blas_geometry_count: wanted
            .max_blas_geometry_count
            .min(available.max_blas_geometry_count),
        max_tlas_instance_count: wanted
            .max_tlas_instance_count
            .min(available.max_tlas_instance_count),
        max_acceleration_structures_per_shader_stage: wanted
            .max_acceleration_structures_per_shader_stage
            .min(available.max_acceleration_structures_per_shader_stage),
        max_buffers_and_acceleration_structures_per_shader_stage: wanted
            .max_buffers_and_acceleration_structures_per_shader_stage
            .min(available.max_buffers_and_acceleration_structures_per_shader_stage),
        max_multiview_view_count: wanted
            .max_multiview_view_count
            .min(available.max_multiview_view_count),
        max_ray_dispatch_count: wanted
            .max_ray_dispatch_count
            .min(available.max_ray_dispatch_count),
        max_ray_recursion_depth: wanted
            .max_ray_recursion_depth
            .min(available.max_ray_recursion_depth),
    }
}
