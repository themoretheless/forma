/**
 * Running the action an event handler names: `state.toggle('open')`, `actions.save()`,
 * `events.saveRequested(id)` or a bare `submit()`.
 */
import { evalAstNode, type FormaEvalScope, parseFormaExprAst } from './forma-expr.ts';
import type { FormaCompileOptions, FormaStateMutator } from './forma-instance.ts';

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
