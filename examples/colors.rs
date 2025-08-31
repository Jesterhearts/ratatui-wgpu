use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use palette::IntoColor;
use palette::Okhsv;
use palette::Srgb;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_wgpu::shaders::CrtPostProcessor;
use ratatui_wgpu::Builder;
use ratatui_wgpu::Dimensions;
use ratatui_wgpu::Font;
use ratatui_wgpu::WgpuBackend;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;
use winit::window::Window;
use winit::window::WindowAttributes;

pub struct App {
    window: Option<Arc<Window>>,
    backend: Option<Terminal<WgpuBackend<'static, 'static, CrtPostProcessor>>>,
    timer: Instant,
    last_frame: Instant,
    durations: [Option<Duration>; 100],
    frame_count: u64,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::builder().build()?;

    let mut app = App {
        window: None,
        backend: None,
        timer: Instant::now(),
        last_frame: Instant::now(),
        durations: [None; 100],
        frame_count: 0,
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
        let duration = self.last_frame.elapsed();
        self.last_frame = Instant::now();
        self.durations[self.frame_count as usize % self.durations.len()] = Some(duration);
        self.frame_count += 1;

        terminal
            .draw(|f| {
                for (idx, cell) in f.buffer_mut().content.iter_mut().enumerate() {
                    let y = idx / width as usize;
                    let x = idx % width as usize;

                    let hsv = Okhsv::new(
                        ((offset + x) % 360) as f32,
                        Okhsv::max_saturation(),
                        (y + 1) as f32 / (height + 1) as f32,
                    );
                    let rgb: Srgb = hsv.into_color();
                    let rgb = rgb.into_format();
                    cell.set_char('█');
                    cell.set_fg(Color::Rgb(rgb.red, rgb.green, rgb.blue));
                }

                let duration = self.durations.iter().flatten().sum::<Duration>();
                let count = self.durations.iter().filter(|d| d.is_some()).count() as u128;

                f.render_widget(
                    Block::bordered().title(format!(
                        "{}x{} -- {:04?}fps",
                        width,
                        height,
                        1_000_000 / duration.as_micros().div_ceil(count)
                    )),
                    f.area(),
                );
            })
            .unwrap();

        self.window.as_ref().unwrap().request_redraw();
    }
}
