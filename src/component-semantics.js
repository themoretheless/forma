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
import { propertyOrigins } from './property-origins.js';

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

function tryFastPattern(pattern, actual, props, state, environment) {
  if (!pattern || typeof pattern !== 'object') {
    const expected = tryFastEvaluate(pattern, props, state, emptyArray, false, environment);
    if (expected === undefined) return undefined;
    return expected === actual;
  }
  if (pattern.expr === '_') return true;
  if (Array.isArray(pattern)) {
    if (!Array.isArray(actual) || pattern.length !== actual.length) return false;
    for (let i = 0; i < pattern.length; i++) {
      const m = tryFastPattern(pattern[i], actual[i], props, state, environment);
      if (m === undefined) return undefined;
      if (!m) return false;
    }
    return true;
  }
  if (own(pattern, 'or')) {
    if (!Array.isArray(pattern.or)) return undefined;
    for (let i = 0; i < pattern.or.length; i++) {
      const m = tryFastPattern(pattern.or[i], actual, props, state, environment);
      if (m === undefined) return undefined;
      if (m) return true;
    }
    return false;
  }
  if (own(pattern, 'comparison')) {
    const cmp = pattern.comparison;
    if (!cmp || typeof actual !== 'number' || !Number.isFinite(actual)) return undefined;
    const rhs = tryFastEvaluate(cmp.value, props, state, emptyArray, false, environment);
    if (typeof rhs !== 'number' || !Number.isFinite(rhs)) return undefined;
    switch (cmp.op) {
      case '<': return actual < rhs;
      case '<=': return actual <= rhs;
      case '>': return actual > rhs;
      case '>=': return actual >= rhs;
      case '==': return actual === rhs;
      case '!=': return actual !== rhs;
      default: return undefined;
    }
  }
  if (own(pattern, 'expression')) {
    return undefined;
  }
  const expected = tryFastEvaluate(pattern, props, state, emptyArray, false, environment);
  if (expected === undefined) return undefined;
  if (expected === actual) return true;
  if (
    expected &&
    actual &&
    typeof expected === 'object' &&
    typeof actual === 'object' &&
    typeof expected.expr === 'string' &&
    typeof actual.expr === 'string'
  ) {
    return expected.expr === actual.expr;
  }
  if (
    (expected === null || typeof expected !== 'object') &&
    (actual === null || typeof actual !== 'object')
  ) {
    return expected === actual;
  }
  return undefined;
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
    const evalProps = tryFastEvaluateProperties(value, props, state, environment);
    if (evalProps === undefined) return undefined;
    const rawChildren = Array.isArray(value.children) ? value.children : emptyArray;
    const evalChildren = new Array(rawChildren.length);
    for (let i = 0; i < rawChildren.length; i++) {
      const child = tryFastEvaluate(rawChildren[i], props, state, stack, true, environment);
      if (child === undefined) return undefined;
      evalChildren[i] = child;
    }
    return {
      ...value,
      props: evalProps,
      children: evalChildren,
    };
  }
  if (own(value, 'match')) {
    const actual = tryFastEvaluate(value.match, props, state, stack, expandElements, environment);
    if (actual === undefined || !Array.isArray(value.branches)) return undefined;
    for (let i = 0; i < value.branches.length; i++) {
      const b = value.branches[i];
      if (!b || own(b, 'guard')) return undefined;
      const matched = tryFastPattern(b.pattern, actual, props, state, environment);
      if (matched === undefined) return undefined;
      if (matched) {
        return tryFastEvaluate(b.value, props, state, stack, expandElements, environment);
      }
    }
    return undefined;
  }
  if (own(value, 'expression') || own(value, 'interpolation')) {
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

function tryFastEvaluateProperties(node, props, state, environment) {
  if (
    !node ||
    node.apply?.length ||
    node.forward?.length ||
    node.matches?.length ||
    (node.bindings && Object.keys(node.bindings).length > 0)
  ) {
    return undefined;
  }
  const rawProps = node.props;
  if (!rawProps) return {};
  const keys = Object.keys(rawProps);
  if (keys.length === 0) return {};
  const out = {};
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i];
    const v = tryFastEvaluate(rawProps[k], props, state, emptyArray, false, environment);
    if (v === undefined) return undefined;
    out[k] = v;
  }
  return out;
}

export function evaluateProperties(node, props, state, environment = emptyObject) {
  const fast = tryFastEvaluateProperties(node, props, state, environment);
  if (fast !== undefined) return fast;
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
  if (Array.isArray(nodes)) {
    const env =
      environment && typeof environment === 'object'
        ? own(environment, 'locals') && environment.locals && typeof environment.locals === 'object'
          ? environment
          : { ...environment, locals: {} }
        : { locals: {} };
    let count = 0;
    const evaluateProps = options.evaluateProps !== false;
    const trackOrigins = Boolean(options.trackOrigins);
    const propertySources = options.propertySources;
    const recursiveOpt = options.recursive;
    function walk(list, depth) {
      if (depth > 64) return undefined;
      const out = new Array(list.length);
      for (let i = 0; i < list.length; i++) {
        const n = list[i];
        if (!n || structuralTypes.has(n.type)) return undefined;
        if (++count > 16384) return undefined;
        const values = evaluateProps ? tryFastEvaluateProperties(n, props, state, env) : (n.props ?? {});
        if (values === undefined) return undefined;
        let args = {};
        if (n.eventArgs) {
          const eventKeys = Object.keys(n.eventArgs);
          if (eventKeys.length > 0) {
            for (let j = 0; j < eventKeys.length; j++) {
              const ek = eventKeys[j];
              const rawArgs = n.eventArgs[ek];
              if (!Array.isArray(rawArgs)) return undefined;
              const evalArgs = new Array(rawArgs.length);
              for (let m = 0; m < rawArgs.length; m++) {
                const av = tryFastEvaluate(rawArgs[m], props, state, emptyArray, false, env);
                if (av === undefined) return undefined;
                evalArgs[m] = av;
              }
              args[ek] = evalArgs;
            }
          }
        }
        let origins;
        if (trackOrigins) {
          if (n.matches?.length || n.forward?.length || (n.bindings && Object.keys(n.bindings).length > 0)) {
            return undefined;
          }
          const sources = n.propertySources ?? emptyObject;
          const raw = n.props ?? emptyObject;
          origins = {};
          const rawKeys = Object.keys(raw);
          for (let j = 0; j < rawKeys.length; j++) {
            const k = rawKeys[j];
            const src = sources[k];
            origins[k] = propertyOrigins(
              raw[k],
              src ? { ...src, label: `${n.type} · экземпляр` } : n.source,
              props,
              propertySources,
            );
          }
        }
        const recurse = typeof recursiveOpt === 'function' ? recursiveOpt(n) : recursiveOpt !== false;
        let children;
        if (!recurse) {
          children = n.children;
        } else {
          const rawChildren = Array.isArray(n.children) ? n.children : emptyArray;
          children = rawChildren.length === 0 ? [] : walk(rawChildren, depth + 1);
          if (children === undefined) return undefined;
        }
        const isSliderNum = n.type === 'Slider' && typeof values.value === 'number';
        const result = {
          ...n,
          props: values,
          eventArgs: args,
          eventArgExpressions: n.eventArgs ?? {},
          environment: env,
        };
        if (origins !== undefined) result.propertyOrigins = origins;
        if (isSliderNum) {
          const path = n.props?.value?.expr;
          result.normalizedRange = Boolean(
            (typeof n.bindings?.value === 'string' && n.bindings.value.length > 0) ||
              (typeof path === 'string' &&
                (path.startsWith('state.') || own(env.locals, path.split('.')[0] ?? '')))
          );
        }
        result.children = children;
        out[i] = result;
      }
      return out;
    }
    const fast = walk(nodes, 0);
    if (fast !== undefined) return fast;
  }
  return wasmExpandStructure(nodes, props, state, environment, options);
}
