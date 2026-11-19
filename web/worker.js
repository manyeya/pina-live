// A Web Worker for Pina Live in a browser (crates/app/src/web.rs): it decodes tracks for the
// decks, or analyses them, with the app's own module, so the page's one thread never stops for
// them. Files come as the browser gave them ("put"); each request is answered, in turn, with
// `out` or `err`.
import init, { worker_decode, worker_analyze } from "./pkg/pina-live.js";

const ready = init();
const files = new Map();
let turn = Promise.resolve();

self.onmessage = ({ data: m }) => {
  if (m.op === "put") {
    files.set(m.path, m.blob);
    return;
  }
  turn = turn.then(() => answer(m));
};

async function answer(m) {
  try {
    await ready;
    const blob = files.get(m.path);
    if (!blob) throw new Error("the file isn't here any more");
    const bytes = new Uint8Array(await blob.arrayBuffer());
    const out = m.op === "decode"
      ? worker_decode(m.path, bytes, m.rate)
      : worker_analyze(m.path, bytes);
    self.postMessage({ id: m.id, out }, [out.buffer]);
  } catch (e) {
    self.postMessage({ id: m.id, err: String((e && e.message) || e) });
  }
}
