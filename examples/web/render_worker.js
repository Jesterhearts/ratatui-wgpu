import init, { render_entrypoint } from "./target/wasm-example.js";

self.onmessage = (event) => {
  let [module, memory, callback, canvas] = event.data;

  init(module, memory)
    .catch((err) => {
      console.log(err);
      throw err;
    })
    .then(async (wasm) => {
      async function anim() {
        await render_entrypoint(callback, canvas);

        requestAnimationFrame(anim);
      }

      requestAnimationFrame(anim);
    });
};
