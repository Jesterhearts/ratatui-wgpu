use std::{
    mem::size_of,
    num::NonZeroU64,
};

use wgpu::{
    self,
    include_wgsl,
    AddressMode,
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
    FilterMode,
    FragmentState,
    LoadOp,
    MultisampleState,
    Operations,
    PipelineCompilationOptions,
    PipelineLayoutDescriptor,
    PrimitiveState,
    PrimitiveTopology,
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
    TextureSampleType,
    TextureViewDimension,
    VertexState,
};

use crate::backend::PostProcessor;

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct Uniforms {
    screen_size: [f32; 2],
    _pad0: [f32; 2],
    use_srgb: u32,
    _pad1: [u32; 7],
}

/// The default post-processor. Used when you don't want to perform any custom
/// shading on the output. This just blits the composited text to the surface.
pub struct DefaultPostProcessor {
    uniforms: Buffer,
    bindings: BindGroupLayout,
    sampler: Sampler,
    pipeline: RenderPipeline,

    blitter: RenderBundle,
}

impl PostProcessor for DefaultPostProcessor {
    type UserData = ();

    fn compile(
        device: &wgpu::Device,
        text_view: &wgpu::TextureView,
        surface_config: &wgpu::SurfaceConfiguration,
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
                entry_point: "vs_main",
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
                entry_point: "fs_main",
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
        device: &wgpu::Device,
        text_view: &wgpu::TextureView,
        surface_config: &wgpu::SurfaceConfiguration,
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
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        _text_view: &wgpu::TextureView,
        surface_config: &wgpu::SurfaceConfiguration,
        surface_view: &wgpu::TextureView,
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
                _pad0: [0.; 2],
                use_srgb: u32::from(surface_config.format.is_srgb()),
                _pad1: [0; 7],
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
            })],
            ..Default::default()
        });

        pass.execute_bundles(Some(&self.blitter));
    }
}

fn build_blitter(
    device: &wgpu::Device,
    layout: &BindGroupLayout,
    text_view: &wgpu::TextureView,
    sampler: &Sampler,
    uniforms: &Buffer,
    surface_config: &wgpu::SurfaceConfiguration,
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
