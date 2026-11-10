import { parse, resolve } from './language.js';

export function createRuntimeCommands(studio, helpers) {
  const { $, hot } = studio;
  const { snapshot, findNode } = helpers;
  return {
    app_run(args) {
      const { files, entry, compiled, state, error, renderer, vectorPreview } = studio;
      if (parse(files[entry]).defaults.contextType) {
        hot.send('forma:form-run', { files });
        return {
          status: 'requested',
          renderer: 'generated-rust',
          backend: args.backend ?? studio.backend ?? 'ask',
          platform: args.platform ?? studio.platform ?? 'desktop',
        };
      }
      if (renderer === 'vector') {
        studio.nativeSource = {
          entry,
          source: files[entry],
          nodes: structuredClone(compiled?.nodes ?? []),
        };
        if (error || !vectorPreview) throw Error(error || 'Векторный renderer не готов');
        const backend = args.backend ?? studio.backend,
          platform = args.platform ?? studio.platform;
        hot.send('forma:vector-run', {
          ...studio.compileComponents(files, entry, state, {
            measureText: vectorPreview.measureText,
            designState:
              studio.scenario < 0 ? null : (compiled?.states[studio.scenario]?.name ?? null),
          }),
          ...(backend && backend !== 'ask' ? { backend } : {}),
          ...(platform && platform !== 'desktop' ? { platform } : {}),
        });
        return {
          status: 'requested',
          renderer,
          backend: backend ?? 'ask',
          platform: platform ?? 'desktop',
        };
      }
      if (error || !compiled) throw Error('Исправьте разметку');
      studio.render(false);
      try {
        hot.send('forma:native-run', {
          files,
          snapshot: studio.nativeSnapshot($('preview'), compiled),
        });
      } finally {
        studio.render();
      }
      return { status: 'requested' };
    },
    app_stop() {
      hot.send('forma:rust-stop', {});
      hot.send('forma:native-stop', {});
      hot.send('forma:vector-stop', {});
      return { status: 'requested' };
    },
    rust_run() {
      if (!hot) throw Error('Requires local dev server');
      hot.send('forma:rust-run', { files: studio.files });
      return {
        status: 'requested',
        note: 'Read events_read for compilation output and exit code',
      };
    },
    rust_stop() {
      hot?.send('forma:rust-stop', {});
      return { status: 'stop requested' };
    },
    state_read() {
      return structuredClone(studio.state);
    },
    state_set(args) {
      const { state } = studio;
      for (const [key, v] of Object.entries(args.values)) {
        if (!Object.hasOwn(state, key) || typeof state[key] !== typeof v)
          throw Error('Unknown field or incompatible type: ' + key);
      }
      Object.assign(state, args.values);
      studio.render();
      studio.output();
      return snapshot();
    },
    event_dispatch(args) {
      const { state, error, mode, renderer, vectorPreview } = studio;
      if (renderer === 'vector') {
        if (error) throw Error(error);
        if (mode !== 'interact') throw Error('Включите взаимодействие');
        const n = findNode(args.start);
        if (!n.events.clicked) throw Error('Выберите кнопку с clicked');
        vectorPreview.activate();
        return snapshot();
      }
      if (error) throw Error('Fix diagnostics first');
      const n = findNode(args.start);
      if (!n.events.clicked) throw Error('No clicked handler');
      if (resolve(n.props.disabled, state)) throw Error('Component is disabled');
      studio.dispatch(n.events.clicked, n);
      return snapshot();
    },
    debug_break(args) {
      $('break').checked = args.enabled;
      $('break').onchange({ target: $('break') });
      return snapshot();
    },
    debug_continue() {
      if (!studio.pending) throw Error('Debugger is not paused');
      $('continue').click();
      return snapshot();
    },
    debug_reset() {
      $('reset').click();
      return snapshot();
    },
    events_read() {
      return [...studio.logs];
    },
    events_clear() {
      studio.logs = [];
      studio.output();
      return snapshot();
    },
    diagnostics_read() {
      return { error: studio.error, entry: studio.entry };
    },
  };
}
