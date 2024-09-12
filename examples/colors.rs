use std::{
    num::NonZeroU32,
    sync::Arc,
    time::Instant,
};

use palette::{
    convert::FromColorUnclamped,
    Okhsv,
    Srgb,
};
use ratatui::{
    prelude::*,
    widgets::*,
};
use ratatui_wgpu::{
    Builder,
    Dimensions,
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

use crate::style::Styled;

pub struct App {
    window: Option<Arc<Window>>,
    backend: Option<Terminal<WgpuBackend<'static, 'static>>>,
    timer: Instant,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::builder().build()?;

    let mut app = App {
        window: None,
        backend: None,
        timer: Instant::now(),
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
                    .with_width_and_height(Dimensions {
                        width: NonZeroU32::new(size.width).unwrap(),
                        height: NonZeroU32::new(size.height).unwrap(),
                    })
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

        let Size { width, height } = terminal.backend().size().unwrap();
        let offset = (self.timer.elapsed().as_millis() / 33) as usize;
        terminal
            .draw(|f| {
                let mut lines = vec![];
                for y in 0..height {
                    lines.push(Line::from(
                        std::iter::repeat("█")
                            .enumerate()
                            .map(|(idx, ch)| {
                                let hsv = Okhsv::new(
                                    ((offset + idx) % 360) as f32,
                                    Okhsv::max_saturation(),
                                    (y + 1) as f32 / (height + 1) as f32,
                                );
                                let rgb = Srgb::from_color_unclamped(hsv);
                                let rgb = rgb.into_format();
                                ch.set_style(
                                    Style::new().fg(Color::Rgb(rgb.red, rgb.green, rgb.blue)),
                                )
                            })
                            .take(width as usize)
                            .collect::<Vec<_>>(),
                    ));
                }
                f.render_widget(Paragraph::new(lines).block(Block::bordered()), f.area());
            })
            .unwrap();

        self.window.as_ref().unwrap().request_redraw();
    }
}
