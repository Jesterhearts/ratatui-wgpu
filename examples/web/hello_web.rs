use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;

use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_wgpu::Builder;
use ratatui_wgpu::Dimensions;
use ratatui_wgpu::Font;
use ratatui_wgpu::WgpuBackend;
use web_sys::HtmlCanvasElement;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;
use winit::platform::web::*;
use winit::window::Window;
use winit::window::WindowAttributes;

pub struct App {
    window: Rc<RefCell<Option<Window>>>,
    backend: Rc<RefCell<Option<Terminal<WgpuBackend<'static, 'static>>>>>,
}

fn main() -> anyhow::Result<()> {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug).unwrap();

    let event_loop = EventLoop::builder().build()?;

    let app = App {
        window: Rc::default(),
        backend: Rc::default(),
    };
    event_loop.spawn_app(app);

    Ok(())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.window = Rc::new(RefCell::new(Some(
            event_loop
                .create_window(WindowAttributes::default())
                .unwrap(),
        )));

        let window = self.window.clone();
        let backend = self.backend.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let (height, width) = web_sys::window()
                .and_then(|win| win.document())
                .and_then(|doc| {
                    let dst = doc.get_element_by_id("glcanvas")?;

                    let canvas: HtmlCanvasElement = window.borrow().as_ref()?.canvas()?;
                    let style = canvas.style();
                    style.set_property("display", "block").ok()?;
                    style.set_property("width", "100%").ok()?;
                    style.set_property("height", "100%").ok()?;
                    style.set_property("position", "absolute").ok()?;
                    style.set_property("top", "0").ok()?;
                    style.set_property("left", "0").ok()?;
                    style.set_property("z-index", "1").ok()?;

                    dst.append_with_node_1(&web_sys::Element::from(canvas.clone()))
                        .ok()?;

                    let bounds = canvas.get_bounding_client_rect();
                    Some((
                        NonZeroU32::new(bounds.height() as u32)?,
                        NonZeroU32::new(bounds.width() as u32)?,
                    ))
                })
                .expect("Failed to attach canvas");

            let canvas = window.borrow().as_ref().unwrap().canvas().unwrap();

            *backend.borrow_mut() = Some(
                Terminal::new(
                    Builder::from_font(
                        Font::new(include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/backend/fonts/CascadiaMono-Regular.ttf"
                        )))
                        .unwrap(),
                    )
                    .with_width_and_height(Dimensions { width, height })
                    .build_with_target(wgpu::SurfaceTarget::Canvas(canvas))
                    .await
                    .unwrap(),
                )
                .unwrap(),
            );
        });
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let mut terminal = self.backend.borrow_mut();
        let Some(terminal) = terminal.as_mut() else {
            return;
        };

        if let WindowEvent::Resized(size) = event {
            terminal.backend_mut().resize(size.width, size.height);
        }

        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new("Hello Web!").block(Block::bordered()),
                    f.area(),
                )
            })
            .unwrap();

        self.window.borrow().as_ref().unwrap().request_redraw();
    }
}
