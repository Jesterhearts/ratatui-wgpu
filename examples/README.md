# Examples

## Building for Web
### Setup
```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

### Build and Run
The following is for the example `hello_web`. To run other examples, replace `hello_web` with the example name.
```
cargo build --release --features web --example hello_web --target wasm32-unknown-unknown
wasm-bindgen --out-name wasm-example --out-dir examples/web/target --target web target/wasm32-unknown-unknown/release/examples/hello_web.wasm
```

Then serve examples/web to a browser. E.g.
```
# cargo install simple-http-server
simple-http-server -i --nocache examples/web
```

#### Additional instructions for `hello_webworker`
You will need to use nightly Rust and confgure RUSTFLAGS to support atomics. In addition, the
resulting binary will need to be served with coop and coep support.
```
# Windows
$env:RUSTFLAGS = "-Ctarget-feature=+atomics,+bulk-memory,+mutable-globals"
```

```
cargo +nightly build -Zbuild-std=std,panic_abort --release --features web --example hello_webworker --target wasm32-unknown-unknown
wasm-bindgen --out-name wasm-example --out-dir examples/web/target --target web target/wasm32-unknown-unknown/release/examples/hello_webworker.wasm
simple-http-server -i --nocache --coop --coep examples/web
```