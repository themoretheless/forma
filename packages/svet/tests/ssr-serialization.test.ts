/** @vitest-environment jsdom */
import { expect, it } from 'vitest';
import { onCleanup } from '../src/engine/reactivity';
import { renderToString, serializeDomNodeToHtml } from '../src/engine';

it('runs request cleanup when a view throws during SSR', () => {
  let cleanups = 0;
  expect(() =>
    renderToString(() => {
      onCleanup(() => {
        cleanups++;
      });
      throw new Error('render failure');
    }),
  ).toThrow('render failure');
  expect(cleanups).toBe(1);
});

it('serializes live form state rather than stale default HTML attributes', () => {
  const root = document.createElement('div');
  root.innerHTML =
    '<input value="old"><input type="checkbox" checked><textarea>old</textarea><select><option selected>A</option><option>B</option></select>';
  root.querySelector('input')!.value = 'new <&"';
  root.querySelector<HTMLInputElement>('[type=checkbox]')!.checked = false;
  root.querySelector('textarea')!.value = '\nnew <&';
  root.querySelector('select')!.selectedIndex = 1;
  const parsed = document.createElement('div');
  parsed.innerHTML = serializeDomNodeToHtml(root);
  expect(parsed.querySelector('input')!.value).toBe('new <&"');
  expect(parsed.querySelector<HTMLInputElement>('[type=checkbox]')!.checked).toBe(false);
  expect(parsed.querySelector('textarea')!.value).toBe('\nnew <&');
  expect(parsed.querySelector('select')!.selectedIndex).toBe(1);
});

it('preserves escaping when current form values contain HTML-like payloads', () => {
  const input = document.createElement('input');
  input.value = '"><img src=x onerror=alert(1)>';
  const parsed = document.createElement('div');
  parsed.innerHTML = serializeDomNodeToHtml(input);
  expect(parsed.querySelectorAll('input')).toHaveLength(1);
  expect(parsed.querySelector('img')).toBeNull();
  expect(parsed.querySelector('input')!.value).toBe(input.value);
});

it('serializes <style> and <script> raw text without HTML-escaping CSS combinators or JS operators', () => {
  const root = document.createElement('div');
  const style = document.createElement('style');
  style.textContent = '.card > .title { content: "a & b"; }';
  const script = document.createElement('script');
  script.textContent = 'if (a < b && c > d) { const s = "</script>"; }';
  root.append(style, script);

  const html = serializeDomNodeToHtml(root);
  expect(html).toContain('<style>.card > .title { content: "a & b"; }</style>');
  expect(html).toContain('if (a < b && c > d)');
  expect(html).toContain('<\\/script>');
  expect(html).not.toContain('<script>if (a < b && c > d) { const s = "</script>"; }</script>');
});
