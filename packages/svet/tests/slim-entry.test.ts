// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
// The slim entry on its own: nothing in this file may import the self-contained entry.
import {
  compileFormaComponent,
  compileFormaToJsModule,
  instantiateFormaAst,
  loadSvetWasm,
  parseFormaSource,
  registerFormaElement,
  signal,
} from '../src/engine/slim';
import { decodeWasmBinary } from '../src/engine/wasm-binary';

const CARD = `component Card {
  state title: String = 'one';
  Column { Text { key: 'title'; text: state.title; } }
}`;

describe('svet-engine/slim', () => {
  it('refuses to start without a WASM core and names the fix', () => {
    expect(() => signal(1)).toThrow(/loadSvetWasm/);
  });

  it('rejects a failed core request before installing anything', async () => {
    await expect(loadSvetWasm(new Response('nope', { status: 404, statusText: 'Not Found' }))).rejects.toThrow(/404/);
    expect(() => signal(1)).toThrow(/loadSvetWasm/);
  });

  it('starts from the standalone binary served with a non-wasm content type', async () => {
    const bytes = decodeWasmBinary();
    await loadSvetWasm(new Response(bytes as BodyInit, { headers: { 'content-type': 'application/octet-stream' } }));
    const count = signal(1);
    count.set(2);
    expect(count()).toBe(2);
    count.dispose();
    // A second load with different bytes would swap the module under live graphs.
    await expect(loadSvetWasm(decodeWasmBinary())).rejects.toThrow(/already initialized/);
  });

  it('instantiates a pre-parsed view without the source parser', () => {
    const view = instantiateFormaAst(parseFormaSource(CARD));
    const host = document.createElement('div');
    const mounted = view.mount(host);
    expect(host.textContent).toBe('one');
    view.state.title.set('two');
    expect(host.textContent).toBe('two');

    expect(() => instantiateFormaAst(CARD as never)).toThrow(/pre-parsed AST/);
    expect(() => view.hotReload(CARD.replace("'one'", "'three'"))).toThrow(/needs the parser/);
    view.hotReload(parseFormaSource(CARD.replace('state.title', "'reloaded'")));
    expect(host.textContent).toBe('reloaded');
    mounted.unmount();
    view.dispose();
  });

  it('accepts source text again once the application uses the source-based API', () => {
    const fromSource = compileFormaComponent(CARD);
    const host = document.createElement('div');
    const mounted = fromSource.mount(host);
    fromSource.hotReload(CARD.replace('state.title', "'from source'"));
    expect(host.textContent).toBe('from source');
    mounted.unmount();
    fromSource.dispose();
  });

  it('explains how to enable 3D tags, then renders them once the module is imported', async () => {
    const SCENE = `component Scene { Column { Scene3D { key: 'viewport'; width: 120; height: 80; } } }`;
    const host = document.createElement('div');
    expect(() => instantiateFormaAst(parseFormaSource(SCENE)).mount(host)).toThrow(/svet-engine\/forma-3d/);

    await import('../src/engine/forma-3d');
    const view = instantiateFormaAst(parseFormaSource(SCENE));
    const mounted = view.mount(host);
    const canvas = view.getElementByKey('viewport');
    expect(canvas?.tagName).toBe('CANVAS');
    expect(canvas?.getAttribute('data-forma-type')).toBe('Scene3D');
    mounted.unmount();
    view.dispose();
  });

  it('lets an application register its own Forma elements', () => {
    const unregister = registerFormaElement('MapViewport', ({ tag, props, registerKey }) => {
      const element = document.createElement('section');
      element.setAttribute('data-forma-type', tag);
      element.textContent = `zoom ${props.zoom}`;
      registerKey('map', element);
      return element;
    });
    const source = `component Shell { Column { MapViewport { zoom: 12; } } }`;
    const view = instantiateFormaAst(parseFormaSource(source));
    const host = document.createElement('div');
    const mounted = view.mount(host);
    expect(view.getElementByKey('map')?.textContent).toBe('zoom 12');
    mounted.unmount();
    view.dispose();

    unregister();
    const plain = instantiateFormaAst(parseFormaSource(source));
    const plainHost = document.createElement('div');
    const plainMounted = plain.mount(plainHost);
    expect(plainHost.querySelector('section')).toBeNull();
    plainMounted.unmount();
    plain.dispose();
    expect(() => registerFormaElement('lowercase', () => document.createElement('div'))).toThrow(/Invalid Forma element name/);
  });

  it('generates modules that import only the AST runtime and can omit the source text', () => {
    const withSource = compileFormaToJsModule(CARD, 'svet-engine/slim');
    expect(withSource).toContain('import { instantiateFormaAst as compileFormaAst } from "svet-engine/slim";');
    expect(withSource).not.toContain('parseFormaSource');
    expect(withSource).toContain(`export const uiSource = ${JSON.stringify(CARD)};`);

    const production = compileFormaToJsModule(CARD, 'svet-engine/slim', { embedSource: false });
    expect(production).toContain('export const uiSource = undefined;');
    expect(production).not.toContain("state title: String = 'one'");
    expect(production).toContain('export const uiAst = ');
  });
});
