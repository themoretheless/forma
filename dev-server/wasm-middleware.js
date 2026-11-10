import { readFile } from 'node:fs/promises';

const VECTOR_FILES = new Set(['/forma.js', '/forma_bg.wasm']);
const TOKENIZER_FILES = new Set(['/tokenizer_wasm.js', '/tokenizer_wasm_bg.wasm']);

function serveUnchangedWasmBundle(allowedNames, baseDirUrl, unavailableMessage) {
  return async (req, res, next) => {
    const name = req.url?.split('?')[0];
    if (!allowedNames.has(name)) return next();
    try {
      const bytes = await readFile(new URL(`.${name}`, baseDirUrl));
      res.setHeader(
        'Content-Type',
        name.endsWith('.wasm') ? 'application/wasm' : 'text/javascript',
      );
      res.setHeader('Cache-Control', 'no-store');
      res.end(bytes);
    } catch {
      res.statusCode = 503;
      res.end(unavailableMessage);
    }
  };
}

// Generated wasm-bindgen modules must be served unchanged, not transformed by Vite.
export function registerWasmMiddlewares(middlewares) {
  middlewares.use(
    '/__forma_vector',
    serveUnchangedWasmBundle(
      VECTOR_FILES,
      new URL('../public/vector-pkg/', import.meta.url),
      'Build crates/forma WASM first',
    ),
  );
  middlewares.use(
    '/__forma_tokenizer',
    serveUnchangedWasmBundle(
      TOKENIZER_FILES,
      new URL('../../tokenizer/playground/public/wasm/', import.meta.url),
      'Tokenizer WASM unavailable',
    ),
  );
}
