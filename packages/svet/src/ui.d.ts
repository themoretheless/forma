declare module '*.ui' {
  import type { FormaProperty, FormaRuntimeInstance, Signal } from './engine';
  export const uiSource: string;
  export default function createFormaView(options?: {
    props?: Record<string, any>;
    locale?: string;
    viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
  }): FormaRuntimeInstance;
}
