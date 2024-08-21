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
4. Reasonable performance.

## Non-goals
1. Builtin-accessibility support.
   - I'm willing to make concessions to enable accessibility (the native version of my game uses
     accesskit), but integrating directly with accessibility libraries is outside the scope of this
     library.

## Known Limitations
1. No support for complex combining sequences.
   - Supporting this falls under #3 (correctness) and is planned.
2. No support for text blinking.
   - I'm open to adding this, but I have no use for it. This complicates the web story slightly,
     since browsers don't support [`std::time`](https://doc.rust-lang.org/std/time/index.html).
3. No cursor rendering.
    - The location of the cursor is tracked, and operations using it should behave as expected, but
      the cursor is not rendered to the screen.

## Dependencies
This crate attempts to be reasonable with its usage of external dependencies, although it is
definitely not minimal
1. ratatui & wgpu: This crate's core purpose is to create a backend for the former using the latter,
   so these are both necessary.
2. bytemuck: Working directly with byte slices is needed to interface with the shader code, and this
   nicely encapsulates work that would otherwise be unsafe.
3. coolor: Honestly I could probably replace this with my own mapping of ANSI codes to colors and
   probably will at some point.
4. indexmap: Used internally to implement an lru heap which has O(1) lookup for entries. This could
   be replaced with a `HashMap<Key, usize>` + `Vec<Value>`, but doing so would require a lot of
   tedious & error prone book keeping when bubbling entries down the heap.
5. log: Integrating with standard logging infrastructure is very useful. This might be replaced with
   tracing, but I'm not going to go without some sort of logging and I'm not going to implement it
   myself.
6. palette: This is currently used for named colors and to implement text dimming. It will be
   replaced with hardcoded named colors and a manual implementation of dimming.
7. rustybuzz: Font shaping and rendering is _hard_ and way out of scope for this library. There will
   always be an external dependency on some other library to do this for me. Rustybuzz happens to be
   (imo) the current best choice.
8. thiserror: I don't want to write the Error trait by hand. I might consider removing this if doing
   so doesn't turn out to be so bad.
9. tiny-skia: I don't want to implement path stroking & filling by hand, and this library is
   reasonably small and well maintained.
10. unicode-width: I need to access the width of characters to figure out row layout and
    implementing this myself seems silly. This is already pulled in by ratatui, so it doesn't really
    increase the size of the dependency tree.

[Crate Badge]: https://img.shields.io/crates/v/ratatui-wgpu?logo=rust&style=flat-square
[Deps.rs Badge]: https://deps.rs/repo/github/jesterhearts/ratatui-wgpu/status.svg?style=flat-square
[Docs Badge]: https://img.shields.io/docsrs/ratatui-wgpu?logo=rust&style=flat-square
[License Badge]: https://img.shields.io/crates/l/ratatui-wgpu?style=flat-square