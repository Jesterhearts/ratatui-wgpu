#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

pub(crate) mod backend;
pub(crate) mod fonts;
pub mod shaders;
pub(crate) mod utils;

pub use ratatui;
use thiserror::Error;
pub use wgpu;

#[macro_use]
extern crate log;

/// Represents the various errors that can occur during operation.
#[derive(Debug, Error)]
pub enum Error {
    /// Backend creation failed because the device request failed.
    #[error("{0}")]
    DeviceRequestFailed(wgpu::RequestDeviceError),
    /// Backend creation failed because creating the surface failed.
    #[error("{0}")]
    SurfaceCreationFailed(wgpu::CreateSurfaceError),
    /// Backend creation failed because wgpu didn't provide an
    /// [`Adapter`](wgpu::Adapter)
    #[error("Failed to get the Adapter from wgpu.")]
    AdapterRequestFailed,
    /// Backend creation failed because the default surface configuration
    /// couldn't be loaded.
    #[error("Failed to get default Surface configuration from wgpu.")]
    SurfaceConfigurationRequestFailed,
}

pub type Result<T> = ::std::result::Result<T, Error>;

pub use backend::{
    builder::Builder,
    wgpu_backend::WgpuBackend,
    PostProcessor,
    Viewport,
};
pub use fonts::{
    Font,
    Fonts,
};
