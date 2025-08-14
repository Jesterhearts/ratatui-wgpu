# ratatui-wgpu
[![Crate Badge]](https://crates.io/crates/ratatui-wgpu)
![Deps.rs Badge](https://deps.rs/repo/github/jesterhearts/ratatui-wgpu)
[![Docs Badge]](https://docs.rs/ratatui-wgpu/latest/ratatui_wgpu/)
![License Badge]

A wgpu based rendering backend for ratatui.

<img src="splash.gif" alt="splash image" width="600">

This started out as a custom rendering backend for a game I'm developing, and I thought I'd make it
available to the broader community as an alternative rendering target for TUI applications. One of
its primary goals is to support serving TUI applications on the web & desktop.

## Alternatives
- [egui_ratatui](https://crates.io/crates/egui_ratatui) uses an egui widget as its backend, allowing
  it to run anywhere egui can run (including the web).
  - Advantages: Egui is significantly more mature than this library and brings with it a host of
    widgets and builtin accessibility support.
  - Disadvantages: You can't run custom shader code against its output.
- [ratzilla](https://crates.io/crates/ratzilla).
  - Advantages: This is more of an all-in-one solution that also supports e.g keyboard input.
  - Disadvantages: It only runs on the web afaict, and it does not support custom shader code.

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
3. Correct text rendering (including shaping, mixed bidi, and combining sequences).
4. Reasonable performance.
   - Realistically, it can easily run at >60fps at 1080p with the default font size. You can run the
     `colors` example to see how it would perform updating every cell in the terminal on every
     frame. On my machine, `colors` runs at ~800fps at 1080p even with the applied CRT shader
     effect.

## Non-goals
1. Builtin-accessibility support.
   - I'm willing to make concessions to enable accessibility (the native version of my game uses
     accesskit), but integrating directly with accessibility libraries is outside the scope of this
     library.

## Known Limitations
1. No cursor rendering.
    - The location of the cursor is tracked, and operations using it should behave as expected, but
      the cursor is not rendered to the screen.
2. Attempting to render more unique (utf8 character * BOLD|ITALIC) characters than can
   fit in the cache in a single draw call will cause incorrect rendering. This is ~3750 characters
   at the default font size with most fonts. If you need more than this, file a bug and I'll do the
   work to make rendering handle an unbounded number of unique characters.

   To put that in perspective, rendering every printable ascii character in every combination of
   styles would take (95 * 4) 380 cache entries or ~10% of the cache.

## Dependencies
This crate attempts to be reasonable with its usage of external dependencies, although it is
definitely not minimal.
1. ratatui & wgpu: This crate's core purpose is to create a backend for the former using the latter,
   so these are both necessary.
2. ahash (optional, default): Inserting into map & set tracking structures during the core rendering
   loop takes up a significant portion of the render time. Using ahash improves overall performance
   by ~3% for the colors example in my profiling (12% of execution time vs 15%).
3. bitvec: During rendering, I need to efficiently track dirty cells which need their
   background/contents repainted. Bitvec is an efficient structure for tracking which cells are
   dirty.
4. bytemuck: Working directly with byte slices is needed to interface with the shader code, and this
   nicely encapsulates work that would otherwise be unsafe.
5. evictor: This is used to implement an LRU cache for text shaping plans. The code for evictor used
   to be part of this crate, but was moved to its own crate for reuse in other projects.
6. indexmap: Used internally to implement an lru heap which has O(1) lookup for entries and to order
   glyphs in target cells for rendering. This could be replaced with a `HashMap<Key, usize>` +
   `Vec<Value>`, but doing so would require a lot of tedious & error prone book keeping when
   bubbling entries down the heap.
7. log: Integrating with standard logging infrastructure is very useful. This might be replaced with
   tracing, but I'm not going to go without some sort of logging.
8. png (optional, default): Some fonts embed png images as raster graphics for characters. The png
   crate is used to decode these images if they are present.
9. raqote: I don't want to implement path stroking & filling by hand and this library supports all
   the gradient modes required to render from a font's COLR table.
10. rustybuzz: Text shaping is _hard_ and way out of scope for this library. There will always be an
   external dependency on some other library to do this for me. Rustybuzz happens to be (imo) the
   current best choice.
11. thiserror: I don't want to write the Error trait by hand. I might consider removing this if
    doing so doesn't turn out to be so bad.
12. unicode-bidi: I don't want to implement the unicode bidi algorithm by hand, and even if I did,
    most of the code would be based on a implementation like this anyways. This performs well enough
    even though cells have to be concatenated into a single string for processing. There are smarter
    ways to to this processing I'm sure, but I'll optimize when I need to.
13. unicode-properties: I need to check if a character is an emoji in order to know how to handle
    foreground colors and bold/italic styles.
14. unicode-width: I need to access the width of characters to figure out row layout and
    implementing this myself seems silly. This is already pulled in by ratatui, so it doesn't really
    increase the size of the dependency tree.
15. web-time: Used for crossplatform (web & native) time support in order to handle text blinking.

[Crate Badge]: https://img.shields.io/crates/v/ratatui-wgpu?logo=rust&style=flat-square
[Deps.rs Badge]: https://deps.rs/repo/github/jesterhearts/ratatui-wgpu/status.svg?style=flat-square
[Docs Badge]: https://img.shields.io/docsrs/ratatui-wgpu?logo=rust&style=flat-square
[License Badge]: https://img.shields.io/crates/l/ratatui-wgpu?style=flat-square
