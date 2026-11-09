import { createRetainedUiProbe } from './retained-ui.mjs';
import { crossCopyBoundaries } from './cross-copy-boundaries.mjs';
import { createRetentionProbe } from './retention.mjs';
import { createCrossCopyOwnerProbe } from './cross-copy-owner.mjs';
import coreModule from 'svet-engine/core.wasm';
import { AsyncLocalStorage } from 'node:async_hooks';
import { parseHTML } from 'linkedom';
import {
  createSsrRuntime,
  assertSsrAsyncContextCarrier,
  SsrTimeoutError,
} from 'svet-engine/server-runtime';
import {
  h,
  signal,
  onCleanup,
  getWasmCore,
  installSvetWasmModule,
  createSsrStreamTransportDecoder,
  For,
  Show,
} from 'svet-engine';
const { window } = parseHTML('<!doctype html><html><body></body></html>');
for (const name of ['document', 'Node', 'Element']) globalThis[name] = window[name];
installSvetWasmModule(coreModule);
const ownerTokenProbe = { active: false, tokens: [] };
let carrierIndex = 0;
const runtime = createSsrRuntime({
  createAsyncContext: () => {
    const index = carrierIndex++,
      storage = new AsyncLocalStorage();
    if (index !== 1) return storage;
    return {
      getStore: () => storage.getStore(),
      run(value, callback) {
        if (ownerTokenProbe.active) ownerTokenProbe.tokens.push(value);
        return storage.run(value, callback);
      },
      exit: callback => storage.exit(callback),
    };
  },
  randomUUID: () => crypto.randomUUID(),
});
const retentionProbe = createRetentionProbe(runtime, ownerTokenProbe);
const retainedUi = createRetainedUiProbe(runtime);
const crossCopyOwners = createCrossCopyOwnerProbe(runtime, coreModule);
const check = (value, message) => {
  if (!value) throw Error(message);
};
const gate = () => {
  let release;
  const promise = new Promise(resolve => (release = resolve));
  return { promise, release };
};
async function streamingMatrix(trace) {
  const rows = [],
    result = { streams: 8, complete: 0, cancel: 0, abort: 0, error: 0, lateRefusals: 0 };
  try {
    for (const codec of ['identity', 'svet-html-ref-v1'])
      for (const mode of ['complete', 'cancel', 'abort', 'error']) {
        const id = trace + 1000 + rows.length,
          wait = gate(),
          controller = new AbortController(),
          reason = Error('controlled Edge failure ' + id),
          named = {};
        const row = {
          id,
          codec,
          mode,
          wait,
          controller,
          reason,
          retired: gate(),
          cleaned: 0,
          core: null,
        };
        rows.push(row);
        const body = runtime.renderToReadableStreamAsync(
          () => {
            row.core = getWasmCore();
            return {
              node: h(
                'main',
                null,
                h('b', null, String(id)),
                h(async () => {
                  const pending = signal(id);
                  onCleanup(pending.dispose);
                  onCleanup(() => {
                    row.cleaned++;
                    row.retired.release();
                  });
                  await wait.promise;
                  if (mode === 'error') throw reason;
                  if (mode === 'cancel' || mode === 'abort') {
                    let refused = false;
                    try {
                      signal(id);
                    } catch {
                      refused = true;
                      result.lateRefusals++;
                    }
                    check(refused, 'Retired Edge continuation reopened request');
                    return document.createElement('p');
                  }
                  check(
                    getWasmCore() === row.core && pending() === id,
                    'Edge async owner crossed cores',
                  );
                  named.count = signal(id);
                  onCleanup(named.count.dispose);
                  return h('p', null, String(id) + '|<&🦀' + 'x'.repeat(512));
                }, null),
              ),
              signals: named,
            };
          },
          { codec, signal: controller.signal },
        );
        row.reader = body.getReader();
        row.closed = row.reader.closed.then(
          () => ({ ok: true }),
          error => ({ error }),
        );
      }
    await Promise.all(
      rows.map(async row => {
        const first = await row.reader.read();
        check(!first.done, 'Missing Edge shell');
        row.shell = JSON.parse(new TextDecoder().decode(first.value));
        check(
          row.shell.payload.type === 'shell' &&
            row.shell.payload.html.includes('<b>' + row.id + '</b>'),
          'Wrong Edge shell identity',
        );
      }),
    );
    check(
      new Set(rows.map(row => row.core)).size === 8 &&
        rows.every(row => row.core.svet_get_node_count() > 0),
      'Edge streaming cores not isolated/live',
    );
    await Promise.all(
      rows
        .filter(row => row.mode === 'cancel' || row.mode === 'abort')
        .map(async row => {
          if (row.mode === 'cancel') {
            const pending = row.reader.read();
            await row.reader.cancel();
            check((await pending).done, 'Edge cancel failed to finish pending read');
          } else {
            row.controller.abort(row.reason);
            check((await row.closed).error === row.reason, 'Edge abort lost reason identity');
          }
          await row.retired.promise;
        }),
    );
    await Promise.resolve();
    await Promise.resolve();
    for (const row of rows)
      check(
        row.core.svet_get_node_count() > 0 === (row.mode === 'complete' || row.mode === 'error'),
        'Edge cancellation corrupted neighbour graph',
      );
    await Promise.all(
      rows.map(async row => {
        if (row.mode === 'error') {
          row.wait.release();
          check(
            (await row.closed).error === row.reason,
            'Background Edge failure not delivered without read',
          );
        } else if (row.mode === 'complete') {
          const decoder = row.codec === 'identity' ? null : createSsrStreamTransportDecoder(),
            chunks = [decoder ? decoder.decode(row.shell) : row.shell];
          row.wait.release();
          for (let i = 0; i < 2; i++) {
            const part = await row.reader.read();
            check(!part.done, 'Missing Edge data frame');
            const frame = JSON.parse(new TextDecoder().decode(part.value));
            if (i === 1 && decoder)
              check(frame.codec === 'svet-html-ref-v1', 'Edge complete failed to use codec');
            chunks.push(decoder ? decoder.decode(frame) : frame);
          }
          check((await row.closed).ok, 'Edge complete did not auto-close');
          decoder?.dispose();
          check(
            chunks.map(c => c.sequence).join(',') === '0,1,2' &&
              chunks.map(c => c.payload.type).join(',') === 'shell,patch,complete',
            'Wrong Edge frame ordering',
          );
          check(
            chunks[2].payload.html ===
              '<main><b>' +
                row.id +
                '</b><p>' +
                row.id +
                '|&lt;&amp;🦀' +
                'x'.repeat(512) +
                '</p></main>',
            'Edge final HTML mismatch',
          );
          check(
            JSON.parse(chunks[2].payload.stateJson).signals.count === row.id,
            'Edge snapshot crossed request values',
          );
        }
        row.wait.release();
        await row.retired.promise;
        await Promise.resolve();
        check(
          row.cleaned === 1 && row.core.svet_get_node_count() === 0,
          'Edge stream graph/owner leaked',
        );
        result[row.mode]++;
      }),
    );
    check(result.lateRefusals === 4, 'Not all retired Edge continuations were refused');
    return result;
  } finally {
    for (const row of rows) row.wait.release();
    await Promise.all(rows.map(row => row.reader?.cancel().catch(() => {})));
  }
}
async function domLifecycleMatrix(trace) {
  const result = {
    cycles: 16,
    keyedMoves: 0,
    removedRows: 0,
    cleanedRoots: 0,
    cleanedRows: 0,
    eventTeardowns: 0,
  };
  for (let cycle = 0; cycle < result.cycles; cycle++) {
    let core,
      rootCleaned = 0,
      rowCleaned = 0,
      button,
      events = 0;
    const id = trace + cycle;
    const rendered = await runtime.renderToStringAsync(async () => {
      core = getWasmCore();
      const items = signal([1, 2, 3]),
        visible = signal(true),
        text = signal('old');
      onCleanup(items.dispose);
      onCleanup(visible.dispose);
      onCleanup(text.dispose);
      onCleanup(() => rootCleaned++);
      const list = h(
        'ul',
        null,
        For({
          each: items,
          keyFn: item => item,
          children: (item, index) => {
            onCleanup(() => rowCleaned++);
            return h('li', { 'data-id': String(item) }, () => String(item) + ':' + index());
          },
        }),
      );
      const initial = Array.from(list.querySelectorAll('li'));
      button = h('button', { onCustom: () => events++ }, 'event');
      button.dispatchEvent(new window.Event('custom'));
      check(events === 1, 'Edge event handler not invoked');
      const node = h(
        'section',
        { 'data-request': String(id) },
        button,
        h('p', null, text),
        list,
        Show({
          when: visible,
          children: () => h('b', null, 'visible'),
          fallback: () => h('i', null, 'hidden'),
        }),
        h('svg', { xmlns: 'http://www.w3.org/2000/svg' }, h('circle', { cx: 3, cy: 4, r: 2 })),
      );
      await Promise.resolve();
      items.set([3, 1, 4]);
      text.set('<&🦀' + id);
      visible.set(false);
      const moved = Array.from(list.querySelectorAll('li'));
      check(
        moved.length === 3 && moved[0] === initial[2] && moved[1] === initial[0],
        'Edge keyed move replaced stable DOM',
      );
      check(
        moved.map(row => row.textContent).join(',') === '3:0,1:1,4:2',
        'Edge keyed indices stale',
      );
      check(rowCleaned === 1 && !initial[1].parentNode, 'Edge removed row scope/DOM survived');
      check(
        node.querySelector('i')?.textContent === 'hidden' && !node.querySelector('b'),
        'Edge Show branch stale',
      );
      check(
        node.querySelector('circle')?.namespaceURI === 'http://www.w3.org/2000/svg',
        'Edge SVG namespace lost',
      );
      result.keyedMoves += 2;
      result.removedRows++;
      return { node };
    });
    check(
      rendered.html.includes('&lt;&amp;🦀' + id) && rendered.html.includes('<i>hidden</i>'),
      'Edge escaped DOM serialization failed',
    );
    check(
      rootCleaned === 1 && rowCleaned === 4 && core.svet_get_node_count() === 0,
      'Edge DOM teardown leaked scopes/graph',
    );
    button.dispatchEvent(new window.Event('custom'));
    check(events === 1, 'Edge event handler survived teardown');
    result.eventTeardowns++;
    result.cleanedRoots += rootCleaned;
    result.cleanedRows += rowCleaned;
  }
  return result;
}
async function formMatrix() {
  let core,
    cleaned = 0;
  const rendered = await runtime.renderToStringAsync(async () => {
    core = getWasmCore();
    const text = signal('start'),
      checked = signal(false),
      choice = signal('a');
    for (const source of [text, checked, choice]) onCleanup(source.dispose);
    onCleanup(() => cleaned++);
    const input = h('input', { model: text }),
      box = h('input', { type: 'checkbox', model: checked }),
      area = h('textarea', { model: text });
    const select = h(
      'select',
      { model: choice },
      h('option', { value: 'a' }, 'A'),
      h('option', { value: 'b' }, 'B'),
    );
    const staticSelect = h(
      'select',
      { value: 'b' },
      h('option', { value: 'a' }, 'A'),
      h('option', { value: 'b' }, 'B'),
    );
    const reactiveSelect = h(
      'select',
      { value: choice },
      h('option', { value: 'a' }, 'A'),
      h('option', { value: 'b' }, 'B'),
    );
    check(
      staticSelect.value === 'b' && reactiveSelect.value === 'a',
      'Edge initial select value lost',
    );
    const lateSelect = h('select', { value: 'late' }, h('option', { value: 'a' }, 'A'));
    const selections = signal(['a', 'c']);
    onCleanup(selections.dispose);
    const multi = h(
      'select',
      { multiple: true, model: selections },
      ...['a', 'b', 'c'].map(value => h('option', { value }, value)),
    );
    check(
      Array.from(multi.options)
        .filter(option => option.selected)
        .map(option => option.value)
        .join(',') === 'a,c',
      'Edge multiple selection lost',
    );
    const node = h(
      'form',
      null,
      input,
      box,
      area,
      select,
      staticSelect,
      reactiveSelect,
      lateSelect,
      multi,
    );
    await Promise.resolve();
    text.set('<&🦀');
    checked.set(true);
    choice.set('b');
    lateSelect.append(h('option', { value: 'late' }, 'Late'));
    await Promise.resolve();
    await Promise.resolve();
    check(lateSelect.value === 'late', 'Edge late option selection stale');
    selections.set(['b']);
    check(
      Array.from(multi.options)
        .filter(option => option.selected)
        .map(option => option.value)
        .join(',') === 'b',
      'Edge multiple reactive update stale',
    );
    multi.options[2].setAttribute('selected', '');
    multi.dispatchEvent(new window.Event('change'));
    check(selections().join(',') === 'b,c', 'Edge multiple change roundtrip lost values');
    check(
      input.value === '<&🦀' &&
        area.value === '<&🦀' &&
        box.checked &&
        select.value === 'b' &&
        reactiveSelect.value === 'b',
      'Edge form properties stale',
    );
    return { node };
  });
  check(
    rendered.html.includes('value="&lt;&amp;🦀"') &&
      rendered.html.includes('checked=""') &&
      rendered.html.includes('<textarea>&lt;&amp;🦀</textarea>') &&
      rendered.html.includes('value="b" selected=""'),
    'Edge form HTML stale',
  );
  check(
    rendered.html.includes(
      '<option value="b" selected="">b</option><option value="c" selected="">c</option>',
    ),
    'Edge multiple HTML lost values',
  );
  check(cleaned === 1 && core.svet_get_node_count() === 0, 'Edge form graph leaked');
  return { controls: 8, multiple: true, lateOptions: true, cleaned, nodes: 0 };
}
const responseRows = new Map();
function edgeResponse(request) {
  const url = new URL(request.url),
    id = url.searchParams.get('id'),
    codec = url.searchParams.get('codec') ?? 'identity',
    mode = url.searchParams.get('mode') ?? 'complete';
  if (url.pathname === '/stream-stats') {
    const row = responseRows.get(id);
    if (!row) return Response.json({ error: 'missing' }, { status: 404 });
    const snapshot = {
      producerFailed: row.producerFailed === true,
      signalAborted: row.signalAborted,
      deadlineAborted: row.deadlineAborted,
      phase: row.phase,
      retiredPhase: row.retiredPhase,
      cleaned: row.cleaned,
      nodes: row.core?.svet_get_node_count(),
    };
    if (url.searchParams.get('forget') === '1') responseRows.delete(id);
    return Response.json(snapshot);
  }
  if (!id || responseRows.has(id) || responseRows.size >= 8)
    return Response.json({ error: 'fixture capacity/id' }, { status: 400 });
  const row = { phase: 'not-started', signalAborted: false, cleaned: 0, core: null };
  responseRows.set(id, row);
  const named = {};
  const lifetime = new AbortController(),
    deadline = Number(url.searchParams.get('deadline') ?? 0);
  const incomingAborted = () => {
    row.signalAborted = true;
    lifetime.abort(request.signal.reason);
  };
  const deadlineTimer =
    deadline > 0 && deadline <= 1000
      ? setTimeout(() => {
          row.deadlineAborted = true;
          lifetime.abort(Error('Host deadline'));
        }, deadline)
      : null;
  const cleanupLifetime = () => {
    request.signal.removeEventListener('abort', incomingAborted);
    if (deadlineTimer !== null) clearTimeout(deadlineTimer);
    lifetime.signal.removeEventListener('abort', cleanupLifetime);
  };
  lifetime.signal.addEventListener('abort', cleanupLifetime, { once: true });
  request.signal.addEventListener('abort', incomingAborted, { once: true });
  if (request.signal.aborted) incomingAborted();
  const body = runtime.renderToReadableStreamAsync(
    () => {
      row.core = getWasmCore();
      return {
        node: h(
          'main',
          null,
          h('b', null, id),
          h(async () => {
            row.phase = 'component-pending';
            const pending = signal(1);
            onCleanup(pending.dispose);
            onCleanup(cleanupLifetime);
            onCleanup(() => {
              row.cleaned++;
              row.retiredPhase = row.phase;
            });
            if (mode === 'cancel')
              await new Promise(resolve => {
                const timer = setTimeout(resolve, 5000);
                onCleanup(() => clearTimeout(timer));
              });
            else await Promise.resolve();
            row.phase = 'component-ready';
            if (mode === 'error') {
              row.producerFailed = true;
              throw Error('controlled response body failure');
            }
            named.count = signal(7);
            onCleanup(named.count.dispose);
            return h('p', null, 'response|<&🦀' + 'x'.repeat(512));
          }, null),
        ),
        signals: named,
      };
    },
    { codec, signal: lifetime.signal },
  );
  return new Response(body, {
    headers: {
      'content-type': 'application/x-ndjson',
      'svet-stream-version': '1',
      'svet-stream-codec': codec,
    },
  });
}
function timeoutReader(view, transport) {
  if (transport === 'chunks') {
    const iterator = runtime.renderToChunksAsync(view, { timeoutMs: 10 });
    return {
      read: () => iterator.next(),
      get closed() {
        return iterator.next();
      },
    };
  }
  return runtime.renderToReadableStreamAsync(view, { timeoutMs: 10 }).getReader();
}
const probeProgress = new Map();
export default {
  async fetch(request) {
    try {
      if (new URL(request.url).pathname === '/retained-ui')
        return Response.json(await retainedUi.run());
      if (new URL(request.url).pathname === '/retained-ui-gc')
        return Response.json(retainedUi.inspect());
      if (new URL(request.url).pathname === '/copy-boundaries')
        return Response.json(await crossCopyBoundaries(runtime));
      if (new URL(request.url).pathname === '/copy-owners')
        return Response.json(
          await crossCopyOwners.run(Number(new URL(request.url).searchParams.get('trace') ?? 0)),
        );
      if (new URL(request.url).pathname === '/copy-gc')
        return Response.json(crossCopyOwners.inspect());
      if (new URL(request.url).pathname.startsWith('/gc/')) return await retentionProbe(request);
      if (new URL(request.url).pathname === '/probe-stats')
        return Response.json(Array.from(probeProgress));
      if (new URL(request.url).pathname.startsWith('/stream')) return edgeResponse(request);
      await assertSsrAsyncContextCarrier(() => new AsyncLocalStorage());
      const trace = Number(new URL(request.url).searchParams.get('trace') ?? 0);
      let cleaned = 0;
      const cores = [];
      probeProgress.set(trace, 'strings');
      if (probeProgress.size > 16) probeProgress.delete(probeProgress.keys().next().value);
      const results = await Promise.all(
        Array.from({ length: 8 }, (_, i) =>
          runtime.renderToStringAsync(() => {
            cores.push(getWasmCore());
            return {
              node: h(async () => {
                await Promise.resolve();
                const value = signal(trace + i);
                onCleanup(() => {
                  value.dispose();
                  cleaned++;
                });
                await Promise.resolve();
                return h('p', null, String(value()));
              }, null),
            };
          }),
        ),
      );
      const html = results.map(result => result.html);
      if (
        html.some((value, i) => value !== `<p>${trace + i}</p>`) ||
        new Set(cores).size !== 8 ||
        cleaned !== 8 ||
        cores.some(core => core.svet_get_node_count() !== 0)
      )
        throw Error('Edge SSR isolation/cleanup failed');
      probeProgress.set(trace, 'timeout');
      const retired = gate();
      let timeoutCore,
        timeoutCleaned = 0;
      const timeoutTransport =
        new URL(request.url).searchParams.get('timeoutTransport') === 'chunks'
          ? 'chunks'
          : 'readable';
      const timeoutView = () => {
        timeoutCore = getWasmCore();
        return {
          node: h(async () => {
            const pending = signal(1);
            onCleanup(pending.dispose);
            onCleanup(() => {
              timeoutCleaned++;
              retired.release();
            });
            await new Promise(() => {});
            return h('p', null);
          }, null),
        };
      };
      const timed = timeoutReader(timeoutView, timeoutTransport);
      probeProgress.set(trace, 'timeout-read');
      await timed.read();
      probeProgress.set(trace, 'timeout-close');
      let timeoutQualified = false;
      let watchdog;
      try {
        if (new URL(request.url).searchParams.has('keepalive'))
          await Promise.race([
            timed.closed,
            new Promise(
              (_, reject) =>
                (watchdog = setTimeout(() => reject(Error('Native host timeout watchdog')), 100)),
            ),
          ]);
        else await timed.closed;
      } catch (error) {
        timeoutQualified = error instanceof SsrTimeoutError && error.timeoutMs === 10;
      } finally {
        clearTimeout(watchdog);
      }
      probeProgress.set(trace, 'timeout-retire');
      await retired.promise;
      check(
        timeoutQualified && timeoutCleaned === 1 && timeoutCore.svet_get_node_count() === 0,
        'Edge runtime timeout failed',
      );
      probeProgress.set(trace, 'streams');
      const streamMatrix = await streamingMatrix(trace);
      probeProgress.set(trace, 'dom');
      const domMatrix = await domLifecycleMatrix(trace);
      probeProgress.set(trace, 'forms');
      const forms = await formMatrix();
      probeProgress.set(trace, 'complete');
      return Response.json({
        status: 'passed',
        carrier: true,
        requests: 8,
        html,
        cleaned,
        timeoutQualified,
        timeoutTransport,
        streamMatrix,
        domMatrix,
        forms,
        gcCapabilities: {
          weakRef: typeof WeakRef === 'function',
          finalizationRegistry: typeof FinalizationRegistry === 'function',
        },
      });
    } catch (error) {
      return Response.json(
        { status: 'failed', error: String(error.stack ?? error) },
        { status: 500 },
      );
    }
  },
};
