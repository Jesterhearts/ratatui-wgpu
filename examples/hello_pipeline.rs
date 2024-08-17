use std::{
    num::{
        NonZeroU32,
        NonZeroU64,
    },
    sync::Arc,
};

use ratatui::{
    widgets::{
        Block,
        Paragraph,
        Wrap,
    },
    Terminal,
};
use ratatui_wgpu::{
    Builder,
    Font,
    PostProcessor,
    WgpuBackend,
};
use wgpu::*;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{
        Window,
        WindowAttributes,
    },
};

pub struct App {
    window: Option<Arc<Window>>,
    backend: Option<Terminal<WgpuBackend<'static, 'static, Pipeline>>>,
}

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct Uniforms {
    use_srgb: u32,
    _pad: [u32; 7],
}

pub struct Pipeline {
    uniforms: Buffer,
    bindings: BindGroupLayout,
    sampler: Sampler,
    pipeline: RenderPipeline,

    shader: RenderBundle,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::builder().build()?;

    let mut app = App {
        window: None,
        backend: None,
    };
    event_loop.run_app(&mut app).unwrap();

    Ok(())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.window = Some(Arc::new(
            event_loop
                .create_window(WindowAttributes::default())
                .unwrap(),
        ));

        let size = self.window.as_ref().unwrap().inner_size();

        self.backend = Some(
            Terminal::new(
                futures_lite::future::block_on(
                    Builder::from_font(Font::new(include_bytes!("assets/Fairfax.ttf")).unwrap())
                        .with_dimensions(
                            NonZeroU32::new(size.width).unwrap(),
                            NonZeroU32::new(size.height).unwrap(),
                        )
                        .build_with_target(self.window.as_ref().unwrap().clone()),
                )
                .unwrap(),
            )
            .unwrap(),
        );

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
            return;
        }

        let Some(terminal) = self.backend.as_mut() else {
            return;
        };

        if let WindowEvent::Resized(size) = event {
            terminal.backend_mut().resize(size.width, size.height);
        }

        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new(LOREM_IPSUM)
                        .wrap(Wrap { trim: false })
                        .block(Block::bordered()),
                    f.area(),
                );
            })
            .unwrap();

        self.window.as_ref().unwrap().request_redraw();
    }
}

impl PostProcessor for Pipeline {
    type UserData = ();

    fn compile(
        device: &wgpu::Device,
        text_view: &wgpu::TextureView,
        surface_config: &wgpu::SurfaceConfiguration,
        _user_data: Self::UserData,
    ) -> Self {
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: None,
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
            label: None,
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

        let shader = device.create_shader_module(include_wgsl!("assets/chromatic_aberration.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
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

        let shader = build_shader(
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
            shader,
        }
    }

    fn resize(
        &mut self,
        device: &wgpu::Device,
        text_view: &wgpu::TextureView,
        surface_config: &wgpu::SurfaceConfiguration,
    ) {
        self.shader = build_shader(
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
                use_srgb: u32::from(surface_config.format.is_srgb()),
                _pad: [0; 7],
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

        pass.execute_bundles(Some(&self.shader));
    }
}

fn build_shader(
    device: &Device,
    layout: &BindGroupLayout,
    text_view: &TextureView,
    sampler: &Sampler,
    uniforms: &Buffer,
    surface_config: &SurfaceConfiguration,
    pipeline: &RenderPipeline,
) -> RenderBundle {
    let bindings = device.create_bind_group(&BindGroupDescriptor {
        label: None,
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
        label: None,
        color_formats: &[Some(surface_config.format)],
        depth_stencil: None,
        sample_count: 1,
        multiview: None,
    });

    encoder.set_pipeline(pipeline);

    encoder.set_bind_group(0, &bindings, &[]);
    encoder.draw(0..3, 0..1);

    encoder.finish(&RenderBundleDescriptor::default())
}

const LOREM_IPSUM: &str =  "
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed nibh lacus, ultrices ac eros eget, bibendum malesuada velit. Pellentesque id leo a sem convallis consectetur. Nulla eget velit pellentesque, dapibus lectus vitae, consequat ex. Maecenas eget accumsan nibh. In at luctus nisl. Sed sapien mauris, placerat sed efficitur et, malesuada sit amet enim. Suspendisse in facilisis massa, sit amet blandit magna.

Donec nec sapien sed metus ullamcorper sodales eu vel erat. Praesent tempor pharetra suscipit. Mauris at augue lorem. Aliquam sit amet rhoncus sapien. Aliquam enim quam, pharetra id nisl et, tincidunt mollis sapien. Morbi volutpat, ante in viverra eleifend, arcu ante congue felis, sit amet laoreet mauris metus a magna. Vivamus placerat erat a nunc pharetra, at dictum massa suscipit. Maecenas sit amet luctus tortor. Nullam lobortis dui ac elit dapibus, ultrices hendrerit orci auctor. Integer at egestas sapien. Aliquam malesuada risus sit amet erat ultricies, ac scelerisque magna viverra. Vivamus rhoncus suscipit nulla eget euismod. Etiam pellentesque consequat dignissim.

Phasellus ut enim non tortor viverra euismod. Vivamus sagittis enim vitae nunc cursus, eget porta ligula condimentum. Suspendisse tortor dolor, blandit at erat sit amet, ultricies varius erat. Nam lobortis sapien lacus, quis semper neque placerat ut. Praesent ut eros enim. Aenean convallis nulla sit amet orci ultricies, eu blandit sem pulvinar. Aenean porta risus arcu. Aenean elementum magna at purus eleifend, in interdum dolor faucibus. Mauris sodales porta massa, posuere bibendum ante pellentesque sit amet. Donec eu turpis lectus. Suspendisse a ultrices neque. Sed non hendrerit risus. Sed ultricies nunc a lectus feugiat laoreet.

Donec ultricies faucibus lectus et placerat. Curabitur tempor lectus id velit gravida, id euismod ex tempus. Nunc aliquam dictum ipsum pellentesque semper. Sed nec metus semper, porta massa consectetur, varius felis. Nullam tempus condimentum diam non posuere. Praesent elit justo, efficitur ut diam sed, tempor tempor odio. In non vestibulum metus. Suspendisse potenti. Donec non purus placerat tortor eleifend tristique. Nulla laoreet sagittis ante, quis tincidunt orci volutpat at. Donec interdum id leo sed interdum. Morbi consectetur arcu quis mi finibus, eget bibendum neque tincidunt. In fringilla orci eros, sit amet ultrices lectus accumsan vitae. Nulla id egestas massa, non laoreet nunc. Curabitur a luctus erat.

Nunc bibendum pretium gravida. Cras porttitor mi in lacus rutrum, a placerat eros dapibus. Phasellus consequat dui nisi, pretium interdum nisl dignissim a. Sed non nisi luctus, aliquet felis hendrerit, pretium nulla. Morbi eget commodo massa. Fusce nisi nulla, varius sit amet velit vitae, lacinia dapibus lorem. Donec pulvinar dolor eu egestas viverra. Nullam a tellus bibendum, commodo mi eget, congue lectus. 
";
