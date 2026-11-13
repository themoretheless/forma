// The preview's debugger: it decides whether an event runs now, waits at a breakpoint or is
// dropped because another one is already waiting. It owns no DOM and no project state; the
// editor supplies those as ports.

/**
 * @param {object} ports
 * @param {(node: object, event: string) => unknown[]} ports.argumentsOf values an event passes on
 * @param {(action: string, args: unknown[]) => void} ports.execute runs an action
 * @param {(text: string) => void} ports.log
 * @param {(action: string) => void} ports.paused an event is now waiting before `action`
 * @param {() => void} ports.resumed nothing is waiting any more
 */
export function createPreviewDebugger({ argumentsOf, execute, log, paused, resumed }) {
  let pending = null,
    breakOn = false;
  return {
    /** The event waiting at the breakpoint, or `null`. */
    get pending() {
      return pending;
    },
    get breakOn() {
      return breakOn;
    },
    setBreak(enabled) {
      breakOn = Boolean(enabled);
    },
    dispatch(action, node, event = 'clicked') {
      if (pending) {
        log('Событие пропущено: debugger приостановлен');
        return;
      }
      const args = argumentsOf(node, event);
      log(
        `Событие ${node.type}.${event} → ${action}(${args.map(value => JSON.stringify(value)).join(', ')})`,
      );
      if (breakOn) {
        pending = { action, n: node, args };
        paused(action);
        return;
      }
      execute(action, args);
    },
    /** Runs the waiting event. Returns whether there was one. */
    resume() {
      if (!pending) return false;
      const waiting = pending;
      pending = null;
      resumed();
      execute(waiting.action, waiting.args ?? []);
      return true;
    },
    /** Drops the waiting event without running it. */
    reset() {
      pending = null;
      resumed();
    },
  };
}
