use std::{
    num::NonZeroU32,
    sync::Arc,
};

use chrono::Local;
use ratatui::{
    prelude::*,
    widgets::*,
};
use ratatui_wgpu::{
    Builder,
    Font,
    WgpuBackend,
};
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
    backend: Option<Terminal<WgpuBackend<'static, 'static>>>,
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
                    Builder::from_font(
                        Font::new(include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/backend/fonts/CascadiaMono-Regular.ttf"
                        )))
                        .unwrap(),
                    )
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
                    Paragraph::new(Line::from(vec![
                        "Hello World!".bold().italic(),
                        format!(" It is {}", Local::now().format("%H:%M:%S.%f")).dim(),
                    ]))
                    .block(Block::bordered()),
                    f.area(),
                );
            })
            .unwrap();

        self.window.as_ref().unwrap().request_redraw();
    }
}
