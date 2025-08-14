use std::{
    mem::size_of,
    num::NonZeroU64,
};

use web_time::Instant;
use wgpu::{
    self,
    include_wgsl,
    AddressMode,
    BindGroup,
    BindGroupDescriptor,
    BindGroupEntry,
    BindGroupLayout,
    BindGroupLayoutDescriptor,
    BindGroupLayoutEntry,
    BindingResource,
    BindingType,
    Buffer,
    BufferBindingType,
    BufferDescriptor,
    BufferUsages,
    Color,
    ColorTargetState,
    ColorWrites,
    CommandEncoder,
    Device,
    Extent3d,
    FilterMode,
    FragmentState,
    LoadOp,
    MultisampleState,
    Operations,
    PipelineCompilationOptions,
    PipelineLayoutDescriptor,
    PrimitiveState,
    PrimitiveTopology,
    Queue,
    RenderBundle,
    RenderBundleDescriptor,
    RenderBundleEncoderDescriptor,
    RenderPassColorAttachment,
    RenderPassDescriptor,
    RenderPipeline,
    RenderPipelineDescriptor,
    Sampler,
    SamplerBindingType,
    SamplerDescriptor,
    ShaderStages,
    StoreOp,
    SurfaceConfiguration,
    Texture,
    TextureDescriptor,
    TextureDimension,
    TextureFormat,
    TextureSampleType,
    TextureUsages,
    TextureView,
    TextureViewDescriptor,
    TextureViewDimension,
    VertexState,
};

use crate::backend::PostProcessor;

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct Uniforms {
    screen_size: [f32; 2],
    preserve_aspect: u32,
    use_srgb: u32,
}

/// The default post-processor. Used when you don't want to perform any custom
/// shading on the output. This just blits the composited text to the surface.
/// This will stretch characters if the render area size falls between multiples
/// of the character size. Use `AspectPreservingDefaultPostProcessor` if you
/// don't want this behavior.
pub struct DefaultPostProcessor<const PRESERVE_ASPECT: bool = false> {
    uniforms: Buffer,
    bindings: BindGroupLayout,
    sampler: Sampler,
    pipeline: RenderPipeline,

    blitter: RenderBundle,
}

/// A default post-processor which preserves the aspect ratio of characters when
/// the render area size falls in between multiples of the character size.
pub type AspectPreservingDefaultPostProcessor = DefaultPostProcessor<true>;

impl<const PRESERVE_ASPECT: bool> PostProcessor for DefaultPostProcessor<PRESERVE_ASPECT> {
    type UserData = ();

    fn compile(
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        _user_data: Self::UserData,
    ) -> Self {
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("Text Blit Uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
            mapped_at_creation: false,
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

        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Text Blit Bindings Layout"),
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
                        min_binding_size: NonZeroU64::new(size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(include_wgsl!("shaders/blit.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Text Blit Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Text Blitter Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: surface_config.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let blitter = build_blitter(
            device,
            &layout,
            text_view,
            &sampler,
            &uniforms,
            surface_config,
            &pipeline,
        );

        Self {
            uniforms,
            bindings: layout,
            sampler,
            pipeline,
            blitter,
        }
    }

    fn resize(
        &mut self,
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
    ) {
        self.blitter = build_blitter(
            device,
            &self.bindings,
            text_view,
            &self.sampler,
            &self.uniforms,
            surface_config,
            &self.pipeline,
        );
    }

    fn process(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        _text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        surface_view: &TextureView,
    ) {
        {
            let mut uniforms = queue
                .write_buffer_with(
                    &self.uniforms,
                    0,
                    NonZeroU64::new(size_of::<Uniforms>() as u64).unwrap(),
                )
                .unwrap();
            uniforms.copy_from_slice(bytemuck::bytes_of(&Uniforms {
                screen_size: [surface_config.width as f32, surface_config.height as f32],
                preserve_aspect: u32::from(PRESERVE_ASPECT),
                use_srgb: u32::from(surface_config.format.is_srgb()),
            }));
        }

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("Text Blit Pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });

        pass.execute_bundles(Some(&self.blitter));
    }
}

fn build_blitter(
    device: &Device,
    layout: &BindGroupLayout,
    text_view: &TextureView,
    sampler: &Sampler,
    uniforms: &Buffer,
    surface_config: &SurfaceConfiguration,
    pipeline: &RenderPipeline,
) -> RenderBundle {
    let bindings = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Text Blit Bindings"),
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(text_view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 2,
                resource: uniforms.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_render_bundle_encoder(&RenderBundleEncoderDescriptor {
        label: Some("Text Blit Pass Encoder"),
        color_formats: &[Some(surface_config.format)],
        depth_stencil: None,
        sample_count: 1,
        multiview: None,
    });

    encoder.set_pipeline(pipeline);

    encoder.set_bind_group(0, &bindings, &[]);
    encoder.draw(0..3, 0..1);

    encoder.finish(&RenderBundleDescriptor {
        label: Some("Text Blit Pass Bundle"),
    })
}

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct CrtUniforms {
    modulate_crt: [f32; 3],
    // All vec3s are padded like vec 4s
    _pad0: f32,
    resolution: [f32; 2],
    brightness: f32,
    modulate_accumulate: f32,
    modulate_blend: f32,
    slow_fade: i32,
    curve_factor: f32,
    ghost_factor: f32,
    scanline_factor: f32,
    corner_radius: f32,
    mask_type: f32,
    mask_strength: f32,
    use_srgb: i32,
    milliseconds: u32,
    _pad1: [f32; 2],
}

/// Settings for the CRT post-processor.
///
/// See struct members for more information on each setting.
#[derive(Debug, Clone)]
pub struct CrtSettings {
    /// How much to increase/reduce the red channel of the CRT effect.
    /// A good range of values is 0.1 to 2.0.
    /// Defaults to 1.0.
    pub modulate_r: f32,
    /// How much to increase/reduce the green channel of the CRT effect.
    /// A good range of values is 0.1 to 2.0.
    /// Defaults to 1.0.
    pub modulate_g: f32,
    /// How much to increase/reduce the blue channel of the CRT effect.
    /// A good range of values is 0.1 to 2.0.
    /// Defaults to 1.0.
    pub modulate_b: f32,
    /// The brightness of the CRT effect.
    /// A good range of values is 0.0 to 0.2.
    /// Defaults to 0.09.
    pub brightness: f32,
    /// How much to curve the screen for the CRT effect.
    /// A good range of values is 0.0 to 2.0.
    /// Defaults to 1.0.
    pub curve_factor: f32,
    /// How much "ghosting" to apply to the CRT effect.
    /// A good range of values is 0.0 to 1.0.
    /// Defaults to 0.15.
    pub ghost_factor: f32,
    /// How strongly to apply the scanline effect.
    /// A good range of values is 0.0 to 2.0.
    /// Defaults to 0.4.
    pub scanline_factor: f32,
    /// The radius of the corner clipping.
    /// A good range of values is 0.0 to 500.0.
    /// Defaults to 210.0.
    pub corner_radius_factor: f32,
    /// The type of mask to apply to the CRT effect.
    /// 1.0 - TV style mask
    /// 2.0 - Apeture-grille style mask
    /// 3.0 - VGA (stretched)
    /// 4.0 - VGA (no stretch)
    /// Other values will disable the mask.
    /// Defaults to 3.0.
    pub mask_type: f32,
    /// How strongly to apply the mask to the CRT effect.
    /// A good range of values is 0.0 to 1.0.
    /// Defaults to 0.2.
    pub mask_strength: f32,
    /// How much to fade between frames for the CRT effect. A value of 1.0 will
    /// result in ghosting from previous frames for animations/screen
    /// transitions.
    /// Defaults to 0.0.
    pub slow_fade: f32,
}

impl Default for CrtSettings {
    fn default() -> Self {
        Self {
            modulate_r: 1.0,
            modulate_g: 1.0,
            modulate_b: 1.0,
            brightness: 0.09,
            curve_factor: 1.0,
            ghost_factor: 0.15,
            scanline_factor: 0.4,
            corner_radius_factor: 210.0,
            mask_type: 3.0,
            mask_strength: 0.2,
            slow_fade: 0.0,
        }
    }
}

/// A post-processor which applies a CRT effect to the output.
pub struct CrtPostProcessor {
    _sampler: Sampler,

    _crt_uniforms_buffer: Buffer,
    crt_pass: RenderBundle,

    blur_x_uniforms: Buffer,
    blur_y_uniforms: Buffer,

    blur_x_dest: TextureView,
    blur_x_pass: RenderBundle,

    blur_y_dest: TextureView,
    blur_y_pass: RenderBundle,

    accumulate_texture_in: Texture,
    accumulate_texture_out: Texture,
    accumulate_view_out: TextureView,

    width: u32,
    height: u32,
    timer: Instant,

    settings: CrtSettings,
}

impl PostProcessor for CrtPostProcessor {
    type UserData = CrtSettings;

    fn compile(
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        user_data: Self::UserData,
    ) -> Self {
        let drawable_width = surface_config.width;
        #[cfg(not(target_arch = "wasm32"))]
        let drawable_height = surface_config.height - 1;
        #[cfg(target_arch = "wasm32")]
        let drawable_height = surface_config.height;

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Texture Sourced Layout"),
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
            ],
        });

        let text_out_as_in_binding = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Text Compositor Output"),
            layout: &texture_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(text_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        let accumulate_texture_in = device.create_texture(&TextureDescriptor {
            label: Some("Accumulate Out A"),
            size: Extent3d {
                width: drawable_width,
                height: drawable_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let accumulate_view_in =
            accumulate_texture_in.create_view(&TextureViewDescriptor::default());

        let accumulate_in_binding = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Accumulate Input"),
            layout: &texture_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&accumulate_view_in),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        let accumulate_texture_out = device.create_texture(&TextureDescriptor {
            label: Some("Accumulate Out B"),
            size: Extent3d {
                width: drawable_width,
                height: surface_config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let accumulate_view_out =
            accumulate_texture_out.create_view(&TextureViewDescriptor::default());

        let crt_uniforms_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("CRT Uniforms buffer"),
            size: size_of::<CrtUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (crt_pipeline, crt_fs_uniforms) = build_crt(
            device,
            surface_config,
            &texture_layout,
            &crt_uniforms_buffer,
        );

        let blur_x_dest = device.create_texture(&TextureDescriptor {
            label: Some("Blur x pass"),
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

        let blur_x_dest = blur_x_dest.create_view(&TextureViewDescriptor::default());

        let blur_y_dest = device.create_texture(&TextureDescriptor {
            label: Some("Blur y pass"),
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

        let blur_y_dest = blur_y_dest.create_view(&TextureViewDescriptor::default());

        let blur_out_as_in_binding = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Blur output binding"),
            layout: &texture_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&blur_y_dest),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        let blur_x_uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("Blur buffer"),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            size: size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
        });

        let blur_y_uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("Blur buffer"),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            size: size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
        });

        let (blur_pipeline, blur_layout) = build_blur(device);

        let blur_x_pass = {
            let mut encoder = device.create_render_bundle_encoder(&RenderBundleEncoderDescriptor {
                label: Some("Blur x text pass"),
                color_formats: &[Some(TextureFormat::Rgba8Unorm)],
                depth_stencil: None,
                sample_count: 1,
                multiview: None,
            });

            encoder.set_pipeline(&blur_pipeline);

            let blur_bindings = device.create_bind_group(&BindGroupDescriptor {
                label: Some("Blur x text bindings"),
                layout: &blur_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(text_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: blur_x_uniforms.as_entire_binding(),
                    },
                ],
            });
            encoder.set_bind_group(0, &blur_bindings, &[]);
            encoder.draw(0..3, 0..1);

            encoder.finish(&RenderBundleDescriptor {
                label: Some("Blur x text render bundle"),
            })
        };

        let blur_y_pass = {
            let mut encoder = device.create_render_bundle_encoder(&RenderBundleEncoderDescriptor {
                label: Some("Blur y pass"),
                color_formats: &[Some(TextureFormat::Rgba8Unorm)],
                depth_stencil: None,
                sample_count: 1,
                multiview: None,
            });

            encoder.set_pipeline(&blur_pipeline);

            let blur_bindings = device.create_bind_group(&BindGroupDescriptor {
                label: Some("Blur y bindings"),
                layout: &blur_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&blur_x_dest),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: blur_y_uniforms.as_entire_binding(),
                    },
                ],
            });
            encoder.set_bind_group(0, &blur_bindings, &[]);
            encoder.draw(0..3, 0..1);

            encoder.finish(&RenderBundleDescriptor {
                label: Some("Blur y render bundle"),
            })
        };

        let crt_pass = {
            let mut encoder = device.create_render_bundle_encoder(&RenderBundleEncoderDescriptor {
                label: Some("CRT Render Bundle Encoder"),
                color_formats: &[Some(surface_config.format), Some(TextureFormat::Rgba8Unorm)],
                depth_stencil: None,
                sample_count: 1,
                multiview: None,
            });

            encoder.set_pipeline(&crt_pipeline);

            encoder.set_bind_group(0, &text_out_as_in_binding, &[]);
            encoder.set_bind_group(1, &accumulate_in_binding, &[]);
            encoder.set_bind_group(2, &blur_out_as_in_binding, &[]);
            encoder.set_bind_group(3, &crt_fs_uniforms, &[]);
            encoder.draw(0..3, 0..1);

            encoder.finish(&RenderBundleDescriptor {
                label: Some("CRT Render Bundle"),
            })
        };

        Self {
            _sampler: sampler,
            _crt_uniforms_buffer: crt_uniforms_buffer,
            crt_pass,
            blur_x_uniforms,
            blur_y_uniforms,
            blur_x_dest,
            blur_x_pass,
            blur_y_dest,
            blur_y_pass,
            accumulate_texture_in,
            accumulate_texture_out,
            accumulate_view_out,
            width: drawable_width,
            height: drawable_height,
            timer: Instant::now(),
            settings: user_data,
        }
    }

    fn resize(
        &mut self,
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
    ) {
        let settings = self.settings.clone();
        let timer = self.timer;

        *self = Self::compile(device, text_view, surface_config, settings);

        self.timer = timer;
    }

    fn process(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        _text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        surface_view: &TextureView,
    ) {
        {
            let mut uniforms = queue
                .write_buffer_with(
                    &self.blur_x_uniforms,
                    0,
                    NonZeroU64::new(size_of::<[f32; 2]>() as u64).unwrap(),
                )
                .unwrap();
            uniforms.copy_from_slice(bytemuck::cast_slice(&[1f32 / self.width as f32, 0.0]));
        }

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Blur x pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.blur_x_dest,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            render_pass.execute_bundles(Some(&self.blur_x_pass));
        }

        {
            let mut uniforms = queue
                .write_buffer_with(
                    &self.blur_y_uniforms,
                    0,
                    NonZeroU64::new(size_of::<[f32; 2]>() as u64).unwrap(),
                )
                .unwrap();
            uniforms.copy_from_slice(bytemuck::cast_slice(&[0.0, 1f32 / self.height as f32]));
        }

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Blur y pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.blur_y_dest,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            render_pass.execute_bundles(Some(&self.blur_y_pass));
        }

        {
            let mut uniforms = queue
                .write_buffer_with(
                    &self._crt_uniforms_buffer,
                    0,
                    NonZeroU64::new(size_of::<CrtUniforms>() as u64).unwrap(),
                )
                .unwrap();

            uniforms.copy_from_slice(bytemuck::bytes_of(&CrtUniforms {
                modulate_crt: [
                    self.settings.modulate_r,
                    self.settings.modulate_g,
                    self.settings.modulate_b,
                ],
                _pad0: 0.,
                brightness: self.settings.brightness,
                modulate_accumulate: 1.,
                modulate_blend: 1.,
                slow_fade: i32::from(self.settings.slow_fade == 1.0),
                resolution: [self.width as f32, self.height as f32],
                curve_factor: self.settings.curve_factor,
                ghost_factor: self.settings.ghost_factor,
                scanline_factor: self.settings.scanline_factor,
                corner_radius: self.settings.corner_radius_factor,
                mask_type: self.settings.mask_type,
                mask_strength: self.settings.mask_strength,
                use_srgb: i32::from(surface_config.format.is_srgb()),
                milliseconds: self.timer.elapsed().as_millis() as u32,
                _pad1: [0.0; 2],
            }));
        }
        self.timer = Instant::now();

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("CRT Pass"),
                color_attachments: &[
                    Some(RenderPassColorAttachment {
                        view: surface_view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::TRANSPARENT),
                            store: StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    Some(RenderPassColorAttachment {
                        view: &self.accumulate_view_out,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                ],
                ..Default::default()
            });

            render_pass.execute_bundles(Some(&self.crt_pass));
        }

        encoder.copy_texture_to_texture(
            self.accumulate_texture_out.as_image_copy(),
            self.accumulate_texture_in.as_image_copy(),
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn needs_update(&self) -> bool {
        self.settings.slow_fade == 1.0
    }
}

fn build_blur(device: &Device) -> (RenderPipeline, BindGroupLayout) {
    let shader = device.create_shader_module(include_wgsl!("shaders/blur.wgsl"));

    let fragment_shader_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Blur Fragment Binding Layout"),
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

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Blur Layout"),
        bind_group_layouts: &[&fragment_shader_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("Blur Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleStrip,
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
        multiview: None,
        cache: None,
    });

    (pipeline, fragment_shader_layout)
}

fn build_crt(
    device: &Device,
    config: &SurfaceConfiguration,
    texture_layout: &BindGroupLayout,
    crt_uniforms_buffer: &Buffer,
) -> (RenderPipeline, BindGroup) {
    let shader = device.create_shader_module(include_wgsl!("shaders/crt.wgsl"));

    let uniforms_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("CRT Fragment Uniforms Binding Layout"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(NonZeroU64::new(size_of::<CrtUniforms>() as u64).unwrap()),
            },
            count: None,
        }],
    });

    let fs_uniforms = device.create_bind_group(&BindGroupDescriptor {
        label: Some("CRT Fragment Uniforms Binding"),
        layout: &uniforms_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: crt_uniforms_buffer.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("CRT Layout"),
        bind_group_layouts: &[
            texture_layout,
            texture_layout,
            texture_layout,
            &uniforms_layout,
        ],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("CRT Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[
                Some(ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                }),
                Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                }),
            ],
        }),
        multiview: None,
        cache: None,
    });

    (pipeline, fs_uniforms)
}
