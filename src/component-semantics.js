import {
  evaluate as wasmEvaluate,
  evaluateProperties as wasmEvaluateProperties,
  selectedProperties as wasmSelectedProperties,
  selectedPropertySources as wasmSelectedPropertySources,
  validateContract as wasmValidateContract,
  validateTypeName as wasmValidateTypeName,
  matchesType as wasmMatchesType,
  expandStructure as wasmExpandStructure,
} from '@themoretheless/tokenizer-forma/component-semantics';

const own = (o, k) => o != null && typeof o === 'object' && Object.hasOwn(o, k);
const emptyObject = Object.freeze({});
const emptyArray = Object.freeze([]);
const structuralTypes = new Set(['Let', 'If', 'Match', 'For']);
const simpleLiteralExpr = /^(?:#[\da-fA-F]{3,8}|-?\d+(?:\.\d+)?(?:px|ms|s|%|rem|em|vw|vh|ch|fr|deg|rad|turn)?|(?:\d+(?:\.\d+)?)?\*|-|content|auto)$/;
const simplePropIdent = /^props\.([A-Za-z_]\w*)$/;

function isTrivialScalar(value) {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true;
  if (typeof value === 'number') return Number.isFinite(value);
  return false;
}

function tryFastEvaluate(value, props, state, stack, expandElements, environment) {
  if (stack.length > 128) return undefined;
  if (isTrivialScalar(value)) return value;
  if (typeof value !== 'object') return undefined;
  if (Array.isArray(value)) {
    const out = new Array(value.length);
    for (let i = 0; i < value.length; i++) {
      const item = tryFastEvaluate(value[i], props, state, stack, expandElements, environment);
      if (item === undefined) return undefined;
      out[i] = item;
    }
    return out;
  }
  if (own(value, 'type')) {
    if (!expandElements) return value;
    return undefined;
  }
  if (own(value, 'match') || own(value, 'expression') || own(value, 'interpolation')) {
    return undefined;
  }
  if (own(value, 'expr')) {
    const expr = value.expr;
    if (typeof expr !== 'string') return undefined;
    if (simpleLiteralExpr.test(expr)) return value;
    if (expr === 'props.font.size' || simplePropIdent.test(expr)) {
      const key = expr === 'props.font.size' ? 'fontSize' : expr.slice(6);
      const locals = environment?.locals;
      if (locals && own(locals, key)) return locals[key];
      if (!props || !own(props, key)) return undefined;
      if (stack.includes(key)) return undefined;
      return tryFastEvaluate(props[key], props, state, [...stack, key], expandElements, environment);
    }
    return undefined;
  }
  const keys = Object.keys(value);
  const out = {};
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i];
    const v = tryFastEvaluate(value[k], props, state, stack, expandElements, environment);
    if (v === undefined) return undefined;
    out[k] = v;
  }
  return out;
}

export function evaluate(value, props, state, stack = emptyArray, expandElements = true, environment = emptyObject) {
  const fast = tryFastEvaluate(value, props, state, stack, expandElements, environment);
  if (fast !== undefined) return fast;
  return wasmEvaluate(value, props, state, stack, expandElements, environment);
}

export function selectedProperties(groups = emptyArray, props, state, environment = emptyObject) {
  if (!groups || groups.length === 0) return {};
  return wasmSelectedProperties(groups, props, state, environment);
}

export function selectedPropertySources(groups = emptyArray, props, state, environment = emptyObject) {
  if (!groups || groups.length === 0) return {};
  return wasmSelectedPropertySources(groups, props, state, environment);
}

export function evaluateProperties(node, props, state, environment = emptyObject) {
  if (
    node &&
    !node.apply?.length &&
    !node.forward?.length &&
    !node.matches?.length &&
    (!node.bindings || Object.keys(node.bindings).length === 0)
  ) {
    const rawProps = node.props;
    if (!rawProps) return {};
    const keys = Object.keys(rawProps);
    if (keys.length === 0) return {};
    const out = {};
    let allFast = true;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      const v = tryFastEvaluate(rawProps[k], props, state, emptyArray, false, environment);
      if (v === undefined) {
        allFast = false;
        break;
      }
      out[k] = v;
    }
    if (allFast) return out;
  }
  return wasmEvaluateProperties(node, props, state, environment);
}

export function validateContract(definitions = emptyObject, values, enums = emptyObject, label = 'component') {
  if (!definitions || Object.keys(definitions).length === 0) return null;
  return wasmValidateContract(definitions, values, enums, label);
}

const builtinTypes = new Set([
  'String', 'Bool', 'Boolean', 'Number', 'Int', 'Float', 'Color', 'Length', 'Duration',
  'Angle', 'Asset', 'List', 'Object', 'Map', 'Any',
]);

export function validateTypeName(type, enums = emptyObject) {
  if (typeof type === 'string') {
    const trimmed = type.trim();
    const base = trimmed.endsWith('?') ? trimmed.slice(0, -1).trim() : trimmed;
    if (builtinTypes.has(base) || own(enums, base)) return null;
  }
  return wasmValidateTypeName(type, enums);
}

export function matchesType(value, type, enums = emptyObject) {
  return wasmMatchesType(value, type, enums);
}

export function expandStructure(nodes, props, state, environment = emptyObject, options = emptyObject) {
  if (Array.isArray(nodes) && nodes.length === 0) return [];
  if (
    Array.isArray(nodes) &&
    options.recursive === false &&
    options.evaluateProps === false &&
    !options.trackOrigins &&
    nodes.length <= 16384
  ) {
    let plain = true;
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      if (!n || structuralTypes.has(n.type) || (n.eventArgs && Object.keys(n.eventArgs).length > 0)) {
        plain = false;
        break;
      }
    }
    if (plain) {
      const env =
        environment && typeof environment === 'object'
          ? own(environment, 'locals') && environment.locals && typeof environment.locals === 'object'
            ? environment
            : { ...environment, locals: {} }
          : { locals: {} };
      return nodes.map(node => {
        const values = node.props ?? {};
        const isSliderNum = node.type === 'Slider' && typeof values.value === 'number';
        const out = {
          ...node,
          props: values,
          eventArgs: {},
          eventArgExpressions: node.eventArgs ?? {},
          environment: env,
          children: node.children,
        };
        if (isSliderNum) {
          const path = node.props?.value?.expr;
          out.normalizedRange = Boolean(
            (typeof node.bindings?.value === 'string' && node.bindings.value.length > 0) ||
              (typeof path === 'string' &&
                (path.startsWith('state.') || own(env.locals, path.split('.')[0] ?? '')))
          );
        }
        return out;
      });
    }
  }
  return wasmExpandStructure(nodes, props, state, environment, options);
}
