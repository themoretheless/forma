interface ImportMetaGlobOptions {
  query?: string;
  import?: string;
  eager?: boolean;
}

interface ImportMetaEnv {
  readonly DEV?: boolean;
  readonly PROD?: boolean;
  readonly MODE?: string;
  [key: string]: unknown;
}

interface ImportMetaHot {
  send(event: string, payload?: unknown): void;
  on(event: string, listener: (data: any) => void): void;
  off(event: string, listener: (data: any) => void): void;
  dispose(cb: () => void): void;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
  readonly hot?: ImportMetaHot;
  glob(pattern: string, options?: ImportMetaGlobOptions): Record<string, string>;
}

interface FileSystemHandle {
  readonly kind: 'file' | 'directory';
  readonly name: string;
}

interface FileSystemFileHandle extends FileSystemHandle {
  readonly kind: 'file';
  getFile(): Promise<File>;
  createWritable(): Promise<{
    write(data: string | BufferSource): Promise<void>;
    close(): Promise<void>;
  }>;
}

interface FileSystemDirectoryHandle extends FileSystemHandle {
  readonly kind: 'directory';
  entries(): AsyncIterableIterator<[string, FileSystemFileHandle | FileSystemDirectoryHandle]>;
  getDirectoryHandle(
    name: string,
    options?: { create?: boolean },
  ): Promise<FileSystemDirectoryHandle>;
  getFileHandle(name: string, options?: { create?: boolean }): Promise<FileSystemFileHandle>;
  removeEntry(name: string, options?: { recursive?: boolean }): Promise<void>;
}

interface Window {
  showDirectoryPicker?: (options?: {
    mode?: 'read' | 'readwrite';
  }) => Promise<FileSystemDirectoryHandle>;
}

interface Navigator {
  readonly userAgentData?: {
    readonly platform?: string;
  };
  readonly gpu?: {
    requestAdapter(options?: unknown): Promise<any>;
    getPreferredCanvasFormat(): string;
  };
}

interface Error {
  diagnostic?: any;
}

declare const GPUBufferUsage: {
  readonly MAP_READ: number;
  readonly MAP_WRITE: number;
  readonly COPY_SRC: number;
  readonly COPY_DST: number;
  readonly INDEX: number;
  readonly VERTEX: number;
  readonly UNIFORM: number;
  readonly STORAGE: number;
  readonly INDIRECT: number;
  readonly QUERY_RESOLVE: number;
};

declare module '*.css' {
  const styles: undefined;
  export default styles;
}

declare module '*?raw' {
  const content: string;
  export default content;
}

declare module '@themoretheless/tokenizer-forma';
