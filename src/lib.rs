//! # Getting Started
//! Check out the [examples](https://github.com/Jesterhearts/ratatui-wgpu/tree/main/examples)
//! for a number of programs using `winit` for both native and web.
//!
//! A [`WgpuBackend`] can be constructed using a [`Builder`] and then provided
//! to a [`Terminal`](ratatui::Terminal). After that, rendering can be done as
//! normal using the ratatui library. If you need custom shader post-processing,
//! see the [`PostProcessor`] trait or the
//! [`DefaultPostProcessor`](shaders::DefaultPostProcessor) implementation for
//! guidance.
//!
//! Here's a short example using winit on native with the default post processor
//! implementation:
//! ```
//! # use std::{
//! #     num::NonZeroU32,
//! #     sync::Arc,
//! # };
//! #
//! # use chrono::Local;
//! # use ratatui::{
//! #     prelude::*,
//! #     widgets::*,
//! # };
//! # use ratatui_wgpu::{
//! #     Builder,
//! #     Font,
//! #     WgpuBackend,
//! # };
//! # use winit::{
//! #     application::ApplicationHandler,
//! #     event::WindowEvent,
//! #     event_loop::EventLoop,
//! #     window::{
//! #         Window,
//! #         WindowAttributes,
//! #     },
//! # };
//! #
//! pub struct App {
//!     window: Option<Arc<Window>>,
//!     backend: Option<Terminal<WgpuBackend<'static, 'static>>>,
//! }
//!
//! impl ApplicationHandler for App {
//!     fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
//!         self.window = Some(Arc::new(
//!             event_loop
//!                 .create_window(WindowAttributes::default())
//!                 .unwrap(),
//!         ));
//!
//!         let size = self.window.as_ref().unwrap().inner_size();
//!
//!         self.backend = Some(
//!             Terminal::new(
//!                 futures_lite::future::block_on(
//!                     Builder::from_font(
//!                         Font::new(include_bytes!(concat!(
//!                             "backend/fonts/CascadiaMono-Regular.ttf"
//!                         )))
//!                         .unwrap(),
//!                     )
//!                     .with_dimensions(
//!                         NonZeroU32::new(size.width).unwrap(),
//!                         NonZeroU32::new(size.height).unwrap(),
//!                     )
//!                     .build_with_target(self.window.as_ref().unwrap().clone()),
//!                 )
//!                 .unwrap(),
//!             )
//!             .unwrap(),
//!         );
//!
//!         self.window.as_ref().unwrap().request_redraw();
//!     }
//!
//!     fn window_event(
//!         &mut self,
//!         event_loop: &winit::event_loop::ActiveEventLoop,
//!         _window_id: winit::window::WindowId,
//!         event: winit::event::WindowEvent,
//!     ) {
//!         if let WindowEvent::CloseRequested = event {
//!             event_loop.exit();
//!             return;
//!         }
//!
//!         let Some(terminal) = self.backend.as_mut() else {
//!             return;
//!         };
//!
//!         if let WindowEvent::Resized(size) = event {
//!             terminal.backend_mut().resize(size.width, size.height);
//!         }
//!
//!         terminal
//!             .draw(|f| {
//!                 f.render_widget(
//!                     Paragraph::new(Line::from("Hello World!")).block(Block::bordered()),
//!                     f.area(),
//!                 );
//!             })
//!             .unwrap();
//!
//!         self.window.as_ref().unwrap().request_redraw();
//!     }
//! }
//! ```
//!
//! # Limitations
//! 1. No cursor rendering.
//!     - The location of the cursor is tracked, and operations using it should
//!       behave as expected, but the cursor is not rendered to the screen.
//! 2. Attempting to render more unique (utf8 character * BOLD|ITALIC)
//!    characters than can fit in the cache in a single draw call will cause
//!    incorrect rendering. This is ~3750 characters at the default font size
//!    with most fonts. If you need more than this, file a bug and I'll do the
//!    work to make rendering handle an unbounded number of unique characters.
//!    To put that in perspective, rendering every printable ascii character in
//!    every combination of styles would take (95 * 4) 380 cache entries or ~10%
//!    of the cache.

pub(crate) mod backend;
pub(crate) mod colors;
pub(crate) mod fonts;
pub(crate) mod graphics;
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

#[cfg(feature = "ahash")]
type RandomState = ahash::RandomState;
#[cfg(not(feature = "ahash"))]
type RandomState = std::hash::RandomState;

pub use backend::{
    builder::Builder,
    wgpu_backend::WgpuBackend,
    PostProcessor,
    RenderSurface,
    RenderTexture,
    Viewport,
};
pub use fonts::{
    Font,
    Fonts,
};
