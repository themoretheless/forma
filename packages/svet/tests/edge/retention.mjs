import { h, signal, onCleanup, getWasmCore } from 'svet-engine';
import { SsrTimeoutError } from 'svet-engine/server-runtime';
const check = (value, message) => {
  if (!value) throw Error(message);
};
const gate = () => {
  let release;
  const promise = new Promise(resolve => (release = resolve));
  return { promise, release };
};
export function createRetentionProbe(runtime, ownerTokenProbe) {
  let held = [],
    controls = [],
    probes = [],
    allocated = false;
  const track = (kind, value) => probes.push({ kind, ref: new WeakRef(value) });
  async function allocate() {
    check(!allocated, 'GC probe already allocated');
    allocated = true;
    ownerTokenProbe.active = true;
    const rows = [];
    for (const codec of ['identity', 'svet-html-ref-v1'])
      for (const mode of ['complete', 'cancel', 'abort', 'error', 'timeout', 'view-timeout'])
        for (let repeat = 0; repeat < 2; repeat++) {
          const id = rows.length,
            wait = gate(),
            retired = gate(),
            abort = new AbortController(),
            marker = { id },
            reason = Error('controlled GC producer error');
          const row = { mode, id, wait, retired, cleaned: 0, core: null, codec };
          rows.push(row);
          track(mode + ':' + 'capture', marker);
          const view = () => {
            check(marker.id === id, 'GC capture identity corrupted');
            row.core = getWasmCore();
            track(mode + ':' + 'core', row.core);
            track(mode + ':' + 'memory', row.core.memory);
            const source = signal(id);
            track(mode + ':' + 'signal', source);
            onCleanup(source.dispose);
            onCleanup(() => {
              row.cleaned++;
              retired.release();
            });
            if (mode === 'view-timeout') return new Promise(() => {});
            const node = h(
              'main',
              null,
              h('b', null, String(id)),
              h(async () => {
                await wait.promise;
                if (mode === 'error') throw reason;
                return h('p', null, String(id));
              }, null),
            );
            track(mode + ':' + 'node', node);
            return { node };
          };
          track(mode + ':' + 'view', view);
          row.stream = runtime.renderToReadableStreamAsync(view, {
            codec,
            signal: abort.signal,
            timeoutMs: mode === 'timeout' || mode === 'view-timeout' ? 100 : undefined,
          });
          row.reader = row.stream.getReader();
          row.closed = row.reader.closed.then(
            () => ({ ok: true }),
            error => ({ error }),
          );
          row.abort = abort;
          row.reason = reason;
        }
    await Promise.all(
      rows.map(async row => {
        try {
          const part = await row.reader.read();
          check(!part.done && row.mode !== 'view-timeout', 'GC request missing shell');
        } catch (error) {
          check(
            row.mode === 'view-timeout' && error instanceof SsrTimeoutError,
            'Unexpected GC shell failure',
          );
        }
      }),
    );
    check(new Set(rows.map(row => row.core)).size === rows.length, 'GC cores not isolated');
    await Promise.all(
      rows.map(async row => {
        if (row.mode === 'cancel') await row.reader.cancel();
        else if (row.mode === 'abort') {
          row.abort.abort('GC caller abort');
          check((await row.closed).error === 'GC caller abort', 'GC abort reason lost');
        } else if (row.mode === 'timeout' || row.mode === 'view-timeout') {
          const failure = (await row.closed).error;
          check(
            failure instanceof SsrTimeoutError && failure.timeoutMs === 100,
            'GC timeout identity lost',
          );
          row.failure = failure;
        } else {
          row.wait.release();
          if (row.mode === 'error') {
            const failure = (await row.closed).error;
            check(failure === row.reason, 'GC error identity lost');
            row.failure = failure;
          } else {
            while (!(await row.reader.read()).done) {}
            check((await row.closed).ok, 'GC completion failed');
          }
        }
        row.wait.release();
        await row.retired.promise;
        check(
          row.cleaned === 1 && row.core.svet_get_node_count() === 0,
          'GC teardown graph/cleanup failed',
        );
      }),
    );
    ownerTokenProbe.active = false;
    held = rows.map(({ stream, reader, failure }) => ({ stream, reader, failure }));
    for (let i = 0; i < 4; i++) {
      const control = { id: i };
      controls.push(control);
      track('liveControl', control);
    }
    return {
      requests: rows.length,
      objects: probes.length,
      retainedTerminalPairs: held.length,
      positiveControls: controls.length,
      ownerTokensHeld: ownerTokenProbe.tokens.length,
      ownerTokensRetired: ownerTokenProbe.tokens.every(
        token =>
          token.closed &&
          token.core === undefined &&
          token.owner === undefined &&
          token.request?.closed &&
          token.request.core === undefined,
      ),
    };
  }
  return async request => {
    const action = new URL(request.url).pathname;
    if (action === '/gc/allocate') return Response.json(await allocate());
    if (action === '/gc/pressure') {
      check(allocated, 'Allocate first');
      for (let round = 0; round < 12; round++) {
        {
          const bytes = new Uint8Array(16 * 1024 * 1024);
          bytes.fill(round);
          const array = Array.from({ length: 500000 }, (_, i) => i + round);
          check(bytes[0] === round && array[100] === round + 100, 'Pressure allocation failed');
        }
        await new Promise(resolve => setTimeout(resolve, 1));
      }
      return Response.json({
        rounds: 12,
        bytesPerRound: 16 * 1024 * 1024,
        numericEntriesPerRound: 500000,
      });
    }
    if (action === '/gc/inspect') {
      const alive = {};
      for (const p of probes) if (p.ref.deref()) alive[p.kind] = (alive[p.kind] ?? 0) + 1;
      return Response.json({
        alive,
        held: held.length,
        controls: controls.length,
        objects: probes.length,
        ownerTokensHeld: ownerTokenProbe.tokens.length,
      });
    }
    if (action === '/gc/release') {
      held = [];
      controls = [];
      return Response.json({ released: true });
    }
    return Response.json({ error: 'Unknown GC action' }, { status: 404 });
  };
}
