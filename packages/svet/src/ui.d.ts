declare module '*.ui' {
  import type { FormaProperty, FormaRuntimeInstance, Signal } from './engine';
  /** The original `.ui` text; `undefined` in production builds, which ship only the pre-parsed AST. */
  export const uiSource: string | undefined;
  export default function createFormaView(options?: {
    props?: Record<string, any>;
    locale?: string;
    viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
  }): FormaRuntimeInstance;
}
