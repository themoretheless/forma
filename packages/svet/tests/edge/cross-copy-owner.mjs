import * as copy from 'svet-engine-copy';
import {
  getWasmCore,
  getCurrentOwner,
  createContext,
  captureReactiveContext,
  getGraphTelemetry,
  onCleanup,
  signal,
  computed,
} from 'svet-engine';
const check = (value, message) => {
  if (!value) throw Error(message);
};
export function createCrossCopyOwnerProbe(runtime, coreModule) {
  copy.installSvetWasmModule(coreModule);
  const independentCores = getWasmCore() !== copy.getWasmCore(),
    targets = [];
  const weak = (kind, value) => targets.push({ kind, ref: new WeakRef(value) });
  const track = core => {
    weak('core', core);
    weak('memory', core.memory);
    return core;
  };
  return {
    inspect() {
      const kinds = {};
      for (const row of targets) kinds[row.kind] = (kinds[row.kind] ?? 0) + 1;
      return {
        targets: targets.length,
        alive: targets.filter(row => row.ref.deref()).length,
        kinds,
      };
    },
    async run(trace) {
      check(independentCores, 'Second engine was aliased to the first');
      let cleaned = 0,
        lateRefusals = 0,
        ownerMatches = 0,
        domBindings = 0,
        reactiveDomUpdates = 0,
        eventTeardowns = 0,
        retainedCallbacks = 0;
      const cores = [];
      await runtime.runWithIsolatedContextAsync(async () => {
        const core = track(getWasmCore());
        cores.push(core);
        const root = getCurrentOwner(),
          context = createContext('default'),
          reason = Error('copy child failure');
        let failure,
          button,
          clicks = 0;
        const value = signal('initial');
        weak('signal', value);
        onCleanup(value.dispose);
        const changed = 'changed|<&' + trace;
        try {
          await runtime.runWithOwnerScopeAsync([[context.id, 'copy-' + trace]], async () => {
            await Promise.resolve();
            const child = getCurrentOwner();
            check(child !== root, 'Child owner equals root');
            copy.withWasmCore(core, () => {
              check(copy.getCurrentOwner() === child, 'Foreign child owner lost');
              ownerMatches++;
              check(copy.useContext(context) === 'copy-' + trace, 'Foreign context lost');
              copy.computed(() => trace);
              copy.onCleanup(() => cleaned++);
              button = copy.h(
                'button',
                {
                  'data-copy': value,
                  onClick: () => {
                    let owner = copy.getCurrentOwner();
                    while (owner && owner !== child) owner = owner.parent;
                    check(
                      owner === child &&
                        copy.getWasmCore() === core &&
                        copy.useContext(context) === 'copy-' + trace,
                      'Foreign click lost captured owner/core/context',
                    );
                    clicks++;
                  },
                },
                value,
              );
              weak('node', button);
              document.body.append(button);
              domBindings++;
            });
            await Promise.resolve();
            value.set(changed);
            check(
              button.textContent === changed && button.getAttribute('data-copy') === changed,
              'Foreign reactive DOM stale',
            );
            reactiveDomUpdates++;
            button.dispatchEvent(new document.defaultView.Event('click', { bubbles: true }));
            check(clicks === 1, 'Foreign click not delivered');
            throw reason;
          });
        } catch (error) {
          failure = error;
        }
        try {
          if (failure !== reason) throw failure ?? Error('Child rejection was missing');
          check(
            cleaned === 1 && getGraphTelemetry().nodes === 1,
            'Foreign rejected child leaked before parent close',
          );
          value.set('late');
          check(
            value() === 'late' &&
              button.textContent === changed &&
              button.getAttribute('data-copy') === changed,
            'Foreign bindings survived child teardown',
          );
          button.dispatchEvent(new document.defaultView.Event('click', { bubbles: true }));
          check(clicks === 1, 'Foreign click survived child teardown');
          eventTeardowns++;
          value.dispose();
          check(getGraphTelemetry().nodes === 0, 'Foreign parent signal leaked');
        } finally {
          button?.remove();
        }
      });
      let retainedButton,
        retainedClicks = 0;
      try {
        await runtime.runWithIsolatedContextAsync(async () => {
          const core = track(getWasmCore());
          cores.push(core);
          const root = getCurrentOwner(),
            captured = captureReactiveContext();
          let release, late;
          const value = signal(0);
          weak('signal', value);
          onCleanup(value.dispose);
          const gate = new Promise(resolve => (release = resolve));
          try {
            await runtime.runWithOwnerScopeAsync([], async () => {
              late = (async () => {
                await gate;
                let refused = false;
                try {
                  copy.withWasmCore(core, () => copy.computed(() => 1));
                } catch (error) {
                  refused = /completed|disposed/.test(String(error));
                }
                check(refused, 'Late foreign child allocation accepted');
                retainedButton.dispatchEvent(
                  new document.defaultView.Event('click', { bubbles: true }),
                );
                check(
                  retainedClicks === 1 && value() === 1 && retainedButton.textContent === '1',
                  'Retained foreign callback failed after child carrier closure',
                );
                reactiveDomUpdates++;
                retainedCallbacks++;
                return refused;
              })();
              await Promise.resolve();
              check(
                captured.run(() => copy.withWasmCore(core, () => copy.getCurrentOwner())) === root,
                'Explicit root override lost inside active child',
              );
              check(
                copy.withWasmCore(core, () => copy.getCurrentOwner()) === getCurrentOwner(),
                'Captured root override poisoned active child',
              );
              ownerMatches++;
              copy.withWasmCore(core, () => {
                retainedButton = copy.h(
                  'button',
                  {
                    onClick: () => {
                      check(
                        getWasmCore() === core,
                        'Captured foreign handler did not select source core',
                      );
                      value.set(v => v + 1);
                      const derived = computed(() => value() * 2);
                      try {
                        check(derived() === 2, 'Foreign captured computed used wrong core');
                      } finally {
                        derived.dispose();
                      }
                      retainedClicks++;
                    },
                  },
                  value,
                );
                weak('node', retainedButton);
                document.body.append(retainedButton);
                domBindings++;
              });
            });
            release();
            check(await late, 'Late foreign child allocation accepted');
            lateRefusals++;
            check(
              copy.withWasmCore(core, () => copy.getCurrentOwner()) === root,
              'Foreign late refusal poisoned parent',
            );
          } finally {
            captured.dispose();
          }
        });
        retainedButton.dispatchEvent(new document.defaultView.Event('click', { bubbles: true }));
        check(retainedClicks === 1, 'Retained successful-child handler survived parent teardown');
        eventTeardowns++;
      } finally {
        retainedButton?.remove();
      }
      await Promise.all(
        [0, 1].map(id =>
          runtime.runWithIsolatedContextAsync(async () => {
            const core = track(getWasmCore());
            cores.push(core);
            await runtime.runWithOwnerScopeAsync([], async () => {
              await Promise.resolve();
              const child = getCurrentOwner();
              copy.withWasmCore(core, () => {
                check(
                  copy.getCurrentOwner() === child,
                  'Concurrent foreign owner crossed requests',
                );
                ownerMatches++;
                copy.computed(() => trace + id);
              });
            });
          }),
        ),
      );
      check(
        new Set(cores).size === 4 && cores.every(core => core.svet_get_node_count() === 0),
        'Cross-copy request graph not retired',
      );
      return {
        status: 'passed',
        independentCores,
        requests: 4,
        ownerMatches,
        contexts: 1,
        failedChildCleanups: cleaned,
        lateRefusals,
        rootOverrides: 1,
        domBindings,
        reactiveDomUpdates,
        eventTeardowns,
        retainedCallbacks,
        nodesAfter: 0,
      };
    },
  };
}
