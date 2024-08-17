# ratatui-wgpu
[![Crate Badge]](https://crates.io/crates/ratatui-wgpu)
![Deps.rs Badge]
[![Docs Badge]](https://docs.rs/ratatui-wgpu/latest/ratatui_wgpu/)
![License Badge]

A wgpu based rendering backend for ratatui.

This started out as a custom rendering backend for a game I'm developing, and I thought I'd make it
available to the broader community as an alternative rendering target for TUI applications. One of
its primary goals is to support serving TUI applications on the web.

## Alternatives
- [egui_ratatui](https://crates.io/crates/egui_ratatui) uses an egui widget as its backend, allowing
  it to run anywhere egui can run (including the web).
  - Advantages: Egui is significantly more mature than this library and brings with it a host of
    widgets and builtin accessibility support.
  - Disadvantages: You can't run custom shader code against its output.

## Goals
The crate has the following goals in order of descending priority.
1. Allow custom shader code to be run against rendered text.
    - See
      [`PostProcessor`](https://docs.rs/ratatui-wgpu/latest/ratatui_wgpu/trait.PostProcessor.html)
      for details. You can also see the implementation of the
      [`shaders::DefaultPostProcessor`](https://docs.rs/ratatui-wgpu/latest/ratatui_wgpu/shaders/struct.DefaultPostProcessor.html)
      or the `hello_pipeline` example for a demonstration of how this works.
2. Target WASM.
    - The `hello_web` example demonstrates its usage for web. `hello_webworker` shows how to use
      this backend to render from a worker thread.
    - You will likely want to enable the `web` feature if you intend to support Firefox.
3. Correct text rendering (including shaping).
    - This library relies on [swash](https://crates.io/crates/swash) for shaping and layout. Swash
      allows this library to handle most things I've tested it on, although it breaks down when
      presented with complex sequences of combining characters (e.g. zalgo text).
4. Reasonable performance.

## Non-goals
1. Builtin-accessibility support.
   - I'm willing to make concessions to enable accessibility (the native version of my game uses
     accesskit), but integrating directly with accessibility libraries is outside the scope of this
     library.

## Known Limitations
1. No support for rtl text.
    - Supporting this falls under goal #3 (correctness), and is planned.
2. No support for text blinking.
   - I'm open to adding this, but I have no use for it. This complicates the web story slightly,
     since browsers don't support [`std::time`](https://doc.rust-lang.org/std/time/index.html).
3. No cursor rendering.
    - The location of the cursor is tracked, and operations using it should behave as expected, but
      the cursor is not rendered to the screen.


[Crate Badge]: https://img.shields.io/crates/v/ratatui-wgpu?logo=rust&style=flat-square
[Deps.rs Badge]: https://deps.rs/repo/github/jesterhearts/ratatui-wgpu/status.svg?style=flat-square
[Docs Badge]: https://img.shields.io/docsrs/ratatui-wgpu?logo=rust&style=flat-square
[License Badge]: https://img.shields.io/crates/l/ratatui-wgpu?style=flat-square