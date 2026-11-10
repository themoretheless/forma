// Values shared by the Studio page, the Vite dev server and the MCP server. Each of the three used
// to spell these lists out on its own; a value added in one place was rejected by the others.

/** UI backends the native app can start with; `ask` shows the chooser. */
export const BACKENDS = ['egui', 'svet', 'ask'];
export const DEFAULT_BACKEND = 'svet';

/** Device chrome the native app emulates. */
export const PLATFORMS = ['desktop', 'ios', 'android'];
export const DEFAULT_PLATFORM = 'desktop';

export const isBackend = value => BACKENDS.includes(value);
export const isPlatform = value => PLATFORMS.includes(value);
