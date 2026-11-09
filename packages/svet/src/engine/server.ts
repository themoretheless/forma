/** Node host for the shared SSR runtime. Import from svet-engine/server. */
import './wasm-embedded.ts';
import {AsyncLocalStorage} from 'node:async_hooks';
import {randomUUID} from 'node:crypto';
import {createSsrRuntime,type SsrRuntimeHost} from './ssr-runtime.ts';
const host:SsrRuntimeHost={createAsyncContext:<T>()=>new AsyncLocalStorage<T>(),randomUUID};
const runtime=createSsrRuntime(host);
export const {runWithIsolatedContextAsync,runWithOwnerScopeAsync,renderToStringAsync,renderToChunksAsync,renderToReadableStreamAsync}=runtime;
export type {SsrRequestOptions,AsyncSsrView,SsrStreamOptions,SsrReadableStreamOptions} from './ssr-runtime.ts';
export {SsrTimeoutError} from './ssr-runtime.ts';
