pub(crate) mod builder;
pub(crate) mod wgpu_backend;

use palette::Srgb;
use ratatui::style::Color;
use wgpu::{
    Adapter,
    BindGroup,
    Buffer,
    BufferDescriptor,
    BufferUsages,
    CommandEncoder,
    Device,
    Extent3d,
    Queue,
    RenderPipeline,
    Surface,
    SurfaceConfiguration,
    SurfaceTexture,
    Texture,
    TextureDescriptor,
    TextureDimension,
    TextureFormat,
    TextureUsages,
    TextureView,
    TextureViewDescriptor,
};

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

mod private {
    use wgpu::Surface;

    use crate::backend::{
        HeadlessSurface,
        HeadlessTarget,
        RenderTarget,
    };

    pub trait Sealed {}

    pub struct Token;

    impl<'s> Sealed for Surface<'s> {}
    impl Sealed for HeadlessSurface {}
    impl Sealed for RenderTarget {}
    impl Sealed for HeadlessTarget {}
}

/// A Texture target that can be rendered to.
pub trait RenderTexture: private::Sealed + Sized {
    /// Gets a [`wgpu::TextureView`] that can be used for rendering.
    fn get_view(&self, _token: private::Token) -> &TextureView;
    /// Presents the rendered result if applicable.
    fn present(self, _token: private::Token) {}
}

impl RenderTexture for RenderTarget {
    fn get_view(&self, _token: private::Token) -> &TextureView {
        &self.view
    }

    fn present(self, _token: private::Token) {
        self.texture.present();
    }
}

impl RenderTexture for HeadlessTarget {
    fn get_view(&self, _token: private::Token) -> &TextureView {
        &self.view
    }
}

/// A surface that can be rendered to.
pub trait RenderSurface<'s>: private::Sealed {
    type Target: RenderTexture;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'s>>;

    fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration>;

    fn configure(&mut self, device: &Device, config: &SurfaceConfiguration, _token: private::Token);

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target>;
}

pub struct RenderTarget {
    texture: SurfaceTexture,
    view: TextureView,
}

impl<'s> RenderSurface<'s> for Surface<'s> {
    type Target = RenderTarget;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'s>> {
        Some(self)
    }

    fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration> {
        self.get_default_config(adapter, width, height)
    }

    fn configure(
        &mut self,
        device: &Device,
        config: &SurfaceConfiguration,
        _token: private::Token,
    ) {
        Surface::configure(self, device, config);
    }

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target> {
        let output = match self.get_current_texture() {
            Ok(output) => output,
            Err(err) => {
                error!("{err}");
                return None;
            }
        };

        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        Some(RenderTarget {
            texture: output,
            view,
        })
    }
}

pub(crate) struct HeadlessTarget {
    view: TextureView,
}

#[derive(Default)]
pub(crate) struct HeadlessSurface {
    pub(crate) texture: Option<Texture>,
    pub(crate) buffer: Option<Buffer>,
    pub(crate) buffer_width: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl RenderSurface<'static> for HeadlessSurface {
    type Target = HeadlessTarget;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'static>> {
        None
    }

    fn get_default_config(
        &self,
        _adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration> {
        Some(SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: TextureFormat::Rgba8Unorm,
            width,
            height,
            present_mode: wgpu::PresentMode::Immediate,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        })
    }

    fn configure(
        &mut self,
        device: &Device,
        config: &SurfaceConfiguration,
        _token: private::Token,
    ) {
        self.texture = Some(device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        }));

        self.buffer_width = config.width * 4;
        self.buffer = Some(device.create_buffer(&BufferDescriptor {
            label: None,
            size: (self.buffer_width * config.height) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
        self.width = config.width;
        self.height = config.height;
    }

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target> {
        self.texture.as_ref().map(|t| HeadlessTarget {
            view: t.create_view(&TextureViewDescriptor::default()),
        })
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

fn build_wgpu_state(device: &Device, drawable_width: u32, drawable_height: u32) -> WgpuState {
    let text_dest = device.create_texture(&TextureDescriptor {
        label: Some("Text Compositor Out"),
        size: Extent3d {
            width: drawable_width.max(1),
            height: drawable_height.max(1),
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
