/**
 * Running the action an event handler names: `state.toggle('open')`, `actions.save()`,
 * `events.saveRequested(id)` or a bare `submit()`.
 */
import {
  evalAstNode,
  FORBIDDEN_MEMBER_KEYS,
  type FormaEvalScope,
  parseFormaExprAst,
} from './forma-expr.ts';
import type {
  FormaCompileOptions,
  FormaRuntimeInstance,
  FormaStateMutator,
} from './forma-instance.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';
import { batch } from './reactivity.ts';

/** What one instance lends its action runner. */
export interface FormaActionContext {
  /** The built-in state mutators; a host action of the same name takes precedence. */
  actions: FormaStateMutator;
  baseScopeWithLocals(locals?: FormaEvalScope['locals']): FormaEvalScope;
  emitEvent(eventName: string, ...args: unknown[]): void;
  options: FormaCompileOptions | undefined;
}

export function createFormaActionRunner({
  actions,
  baseScopeWithLocals,
  emitEvent,
  options,
}: FormaActionContext) {
  return (actionStr: string, locals?: Record<string, any>, domEvent?: Event) => {
    const trimmed = actionStr.trim();
    const callScope = baseScopeWithLocals(locals);

    // Parse action expression via `parseFormaExprAst` so commas inside strings `'Иванов, И.И.'`
    // or arrays `['a', 'b']` or nested calls `clamp(1, 2, 3)` are handled with 100% accuracy!
    const ast = parseFormaExprAst(trimmed);
    if (ast.type === 'call' && ast.callee.type === 'member' && ast.callee.object.type === 'ident') {
      const targetNamespace = ast.callee.object.name;
      const methodName = ast.callee.property;
      const parsedArgs = ast.args.map(argNode => evalAstNode(argNode, callScope));

      if (targetNamespace === 'state' || targetNamespace === 'actions') {
        const customFn = options?.actions?.[methodName];
        if (typeof customFn === 'function') {
          customFn(...parsedArgs, domEvent);
        } else {
          const method = methodName as keyof FormaStateMutator;
          if (typeof actions[method] === 'function') {
            (actions[method] as (...a: any[]) => void)(...parsedArgs);
          }
        }
        options?.onAction?.(methodName, parsedArgs, domEvent);
        emitEvent(methodName, ...parsedArgs, domEvent);
        return;
      }

      if (targetNamespace === 'events') {
        options?.onAction?.(methodName, parsedArgs, domEvent);
        emitEvent(methodName, ...parsedArgs);
        return;
      }
    } else if (ast.type === 'call' && ast.callee.type === 'ident') {
      const methodName = ast.callee.name;
      const parsedArgs = ast.args.map(argNode => evalAstNode(argNode, callScope));
      options?.actions?.[methodName]?.(...parsedArgs, domEvent);
      options?.onAction?.(methodName, parsedArgs, domEvent);
      emitEvent(methodName, ...parsedArgs);
    }
  };
}

/** What one instance lends its state mutators. */
export interface FormaStateContext {
  /** Returns the signal of a state field, creating it with `initialVal` when it is not declared. */
  ensureStateSignal(key: string, initialVal?: unknown): FormaRuntimeInstance['state'][string];
  stateSignals: FormaRuntimeInstance['state'];
  /** Declared defaults, restored by `reset`. */
  initialStateValues: Record<string, unknown>;
}

/** The mutators `.ui` handlers call as `state.toggle('open')`, `state.increment('count', 1)`. */
export function createFormaStateMutator({
  ensureStateSignal,
  stateSignals,
  initialStateValues,
}: FormaStateContext): FormaStateMutator {
  return {
    toggle(field: string) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, false).update(v => !v);
    },
    set(field: string, value: any) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, value).set(value);
    },
    increment(field: string, delta = 1) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, 0).update(v => Number(v || 0) + delta);
    },
    decrement(field: string, delta = 1) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, 0).update(v => Number(v || 0) - delta);
    },
    push(field: string, item: any) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, []).update(arr => (Array.isArray(arr) ? [...arr, item] : [item]));
    },
    remove(field: string, index: number) {
      const sig = stateSignals[field];
      if (sig) {
        sig.update(arr => (Array.isArray(arr) ? arr.filter((_, idx) => idx !== index) : []));
      }
    },
    reset(field?: string) {
      if (field) {
        const targetSig = stateSignals[field];
        if (targetSig && field in initialStateValues) {
          targetSig.set(structuredCloneIfObject(initialStateValues[field]));
        }
      } else {
        batch(() => {
          for (const [k, initV] of Object.entries(initialStateValues)) {
            stateSignals[k]?.set(structuredCloneIfObject(initV));
          }
        });
      }
    },
  };
}
