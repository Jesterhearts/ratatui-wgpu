use std::cell::OnceCell;
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;

use crossbeam_queue::SegQueue;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_wgpu::Builder;
use ratatui_wgpu::Dimensions;
use ratatui_wgpu::Font;
use ratatui_wgpu::WgpuBackend;
use web_sys::HtmlCanvasElement;
use web_sys::OffscreenCanvas;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;
use winit::platform::web::*;
use winit::window::Window;
use winit::window::WindowAttributes;

enum Command {
    Resize { height: u32, width: u32 },
}
static COMMANDS: SegQueue<Command> = SegQueue::new();

type CallbackFn = dyn Fn(&mut Terminal<WgpuBackend<'static, 'static>>) + Send + 'static;

struct Callback {
    inner: Box<CallbackFn>,
}

pub struct App {
    window: Rc<RefCell<Option<Window>>>,
}

/// The initialization code in this function looks very odd, but it is done this
/// way on purpose.
/// 1. Wgpu types cannot be transfered between threads on wasm32, so they must
///    be initialized in this callback in the worker thread.
/// 2. Failing to return from this function to the `onmessage` invocation
///    prevents the offscreen canvas contents from being displayed to the main
///    canvas. This means the function must be able to be called multiple times.
/// 3. Attempting to initialize using something like
///    `futures_lite::future::block_on` in the thread local context will cause
///    initialization to hang on Chrome.
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn render_entrypoint(ptr: u32, canvas: wasm_bindgen::JsValue) {
    thread_local! {
        static TERMINAL: Rc<OnceCell<RefCell<Terminal<WgpuBackend<'static, 'static>>>>> = Rc::new(OnceCell::new());
    }

    TERMINAL
        .with(|t| {
            let t = t.clone();
            async move {
                let terminal = if let Some(t) = t.get() {
                    t
                } else {
                    let canvas = OffscreenCanvas::from(canvas);

                    t.set(RefCell::new(
                        Terminal::new(
                            Builder::from_font(
                                Font::new(include_bytes!(concat!(
                                    env!("CARGO_MANIFEST_DIR"),
                                    "/src/backend/fonts/CascadiaMono-Regular.ttf"
                                )))
                                .unwrap(),
                            )
                            .with_width_and_height(Dimensions {
                                width: NonZeroU32::new(canvas.width()).unwrap(),
                                height: NonZeroU32::new(canvas.height()).unwrap(),
                            })
                            .build_with_target(wgpu::SurfaceTarget::OffscreenCanvas(canvas))
                            .await
                            .unwrap(),
                        )
                        .unwrap(),
                    ))
                    .ok()
                    .expect("Failed to set terminal");

                    t.get().unwrap()
                };

                // SAFETY: This pointer has been leaked from the main thread
                // and transfered to us. No one else owns a mutable reference
                // to this value. If (somehow), multiple calls to `onmessage`
                // are made while this future is suspended, we aren't creating
                // a mutable reference and causing aliasing issues.
                //
                // It's probably(?) safe to take a mutable reference if you need a FnMut. I'm
                // not an expert though.
                let callback: &Callback = unsafe { &*(ptr as *const Callback) };

                while let Some(message) = COMMANDS.pop() {
                    let Command::Resize { height, width } = message;
                    terminal.borrow_mut().backend_mut().resize(width, height);
                }
                (callback.inner)(&mut terminal.borrow_mut());
            }
        })
        .await;
}

fn main() -> anyhow::Result<()> {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug).unwrap();

    let event_loop = EventLoop::builder().build()?;

    let app = App {
        window: Rc::default(),
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
        wasm_bindgen_futures::spawn_local(async move {
            web_sys::window()
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

                    Some(())
                })
                .expect("Failed to attach canvas");

            let canvas = window
                .borrow()
                .as_ref()
                .unwrap()
                .canvas()
                .unwrap()
                .transfer_control_to_offscreen()
                .unwrap();

            let draw_fn = Box::new(|terminal: &mut Terminal<WgpuBackend<'static, 'static>>| {
                terminal
                    .draw(|f| {
                        f.render_widget(
                            Paragraph::new("♫ Hello from the other side ♫")
                                .block(Block::bordered()),
                            f.area(),
                        )
                    })
                    .unwrap();
            }) as Box<CallbackFn>;

            use wasm_bindgen::JsCast;
            let options = web_sys::WorkerOptions::new();
            options.set_name("render thread");
            options.set_type(web_sys::WorkerType::Module);

            let _worker = std::rc::Rc::new(
                web_sys::Worker::new_with_options("./render_worker.js", &options).unwrap(),
            );
            let mut c_worker = Some(_worker.clone());

            let closure =
                wasm_bindgen::closure::Closure::new(move |_event: web_sys::MessageEvent| {
                    c_worker.take();
                });
            _worker.set_onmessage(Some(closure.as_ref().unchecked_ref()));
            closure.forget();

            let callback = Box::into_raw(Box::new(Callback { inner: draw_fn }));

            let init = js_sys::Array::new();
            init.push(&wasm_bindgen::module());
            init.push(&wasm_bindgen::memory());
            init.push(&wasm_bindgen::JsValue::from(callback as u32));
            init.push(&canvas);

            let transfer = js_sys::Array::new();
            transfer.push(&canvas);

            if let Err(err) = _worker.post_message_with_transfer(&init, &transfer) {
                unsafe {
                    drop(Box::<Callback>::from_raw(callback));
                }

                panic!("Failed to spawn render worker: {err:?}");
            }
        });
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let WindowEvent::Resized(size) = event {
            COMMANDS.push(Command::Resize {
                height: size.height,
                width: size.width,
            });
        }
    }
}
