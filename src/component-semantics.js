// Semantics shared by the component compiler and the HTML preview. Expressions
// are interpreted, never executed as JavaScript.
import {evaluateExpression,matchesPattern} from './expressions.js';
import {propertyOrigins} from './property-origins.js';

const own = (object, key) => Object.hasOwn(object, key);
const scalarTypes = new Set(['String', 'Bool', 'Boolean', 'Number', 'Int', 'Float', 'Color', 'Length', 'Duration', 'Asset']);

export function evaluate(value, props, state, stack = [], expandElements = true, environment = {}) {
  function reference(raw, {optional = false} = {}) {
    const negative = raw.startsWith('!');
    const path = negative ? raw.slice(1) : raw;
    let result;
    if (path.startsWith('props.')) {
      const tail = path.slice(6);
      const parts = tail === 'font.size' ? ['fontSize'] : own(props, tail) ? [tail] : tail.split('.');
      const key = parts.shift();
      if (!own(props, key)) {
        if (optional) return null;
        throw Error(`Неизвестное свойство ${path}`);
      }
      if (stack.includes(key)) throw Error(`Циклическая ссылка props: ${[...stack, key].join(' → ')}`);
      result = evaluate(props[key], props, state, [...stack, key], expandElements, environment);
      result = readPath(result, parts, path, optional);
    } else if (path.startsWith('state.')) {
      result = readPath(state, path.slice(6).split('.'), path, optional || environment.allowMissingState === true);
    } else if (path.startsWith('base.')) {
      throw Error(`${path} допустим только внутри override`);
    } else {
      const [head, ...parts] = path.split('.');
      if (own(environment.locals ?? {}, head)) result = readPath(environment.locals[head], parts, path, optional);
      else if (own(environment.enums ?? {}, head)) {
        if (parts.length !== 1 || !environment.enums[head].includes(parts[0])) throw Error(`Неизвестный вариант ${path}`);
        result = path;
      } else result = {expr: path};
    }
    if (negative) {
      if (typeof result !== 'boolean') throw Error('! ожидает boolean');
      return !result;
    }
    return result;
  }
  if (value?.type) {
    if (!expandElements) return value;
    return {...value, props: evaluateProperties(value, props, state, environment), children: (value.children ?? []).map(child => evaluate(child, props, state, stack, true, environment))};
  }
  // Preserve typed element values inside expression match branches. The generic
  // interpreter calls back for references; element expansion belongs to linking.
  if (value && typeof value === 'object' && own(value, 'match')) {
    const subject = evaluate(value.match, props, state, stack, expandElements, environment);
    const branch = value.branches.find(branch => patternMatches(branch.pattern, subject, props, state, environment));
    if (!branch) throw Error('match: нет подходящей ветки; добавьте _ => …');
    return evaluate(branch.value, props, state, stack, expandElements, environment);
  }
  if (Array.isArray(value)) return value.map(item => evaluate(item, props, state, stack, expandElements, environment));
  if (value && typeof value === 'object' && !value.expr && !value.expression && !value.interpolation) {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, evaluate(item, props, state, stack, expandElements, environment)]));
  }
  return evaluateExpression(value, reference);
}

function readPath(object, parts, path, optional) {
  for (const part of parts) {
    if (object == null || !own(Object(object), part)) {
      if (optional) return null;
      throw Error(`Нет значения ${path}`);
    }
    object = object[part];
  }
  return object;
}

function patternMatches(pattern, value, props, state, environment) {
  return matchesPattern(pattern,value,p=>evaluate(p,props,state,[],false,environment));
}

export function selectedProperties(groups = [], props, state, environment = {}) {
  const result = {};
  for (const group of groups) {
    const subject = evaluate(group.match, props, state, [], false, environment);
    const branch = group.branches.find(branch => patternMatches(branch.pattern, subject, props, state, environment));
    if (!branch) throw Error('match: нет подходящей ветки; добавьте _ => …');
    for (const [key, value] of Object.entries(branch.props)) {
      if (own(result, key)) throw Error(`Несколько групп match задают ${key}`);
      result[key] = value;
    }
  }
  return result;
}

export function selectedPropertySources(groups = [], props, state, environment = {}) {
  const sources={};
  for(const group of groups){
    const subject=evaluate(group.match,props,state,[],false,environment);
    const branch=group.branches.find(branch=>patternMatches(branch.pattern,subject,props,state,environment));
    if(branch)Object.assign(sources,branch.propertySources??{});
  }
  return sources;
}

export function evaluateProperties(node, props, state, environment = {}) {
  const forwarded = {};
  for (const key of node.forward ?? []) {
    if (!own(props, key)) throw Error(`forward: неизвестное свойство props.${key}`);
    forwarded[key] = {expr: `props.${key}`};
  }
  const values = {...forwarded, ...selectedProperties(node.matches, props, state, environment), ...node.props};
  for (const [key, path] of Object.entries(node.bindings ?? {})) {
    if (own(values, key)) throw Error(`Свойство ${key} одновременно имеет значение и двустороннюю привязку`);
    values[key] = {expr: path};
  }
  return Object.fromEntries(Object.entries(values).map(([key, value]) => [key, evaluate(value, props, state, [], false, environment)]));
}

export function validateContract(definitions = {}, values, enums = {}, label = 'component') {
  for (const [name, definition] of Object.entries(definitions)) {
    validateTypeName(definition.type, enums);
    if (!own(values, name)) {
      if (definition.required) throw Error(`${label}: обязательное свойство ${name} не задано`);
      continue;
    }
    if (!matchesType(values[name], definition.type, enums)) throw Error(`${label}.${name}: ожидался ${definition.type}`);
  }
}

export function validateTypeName(type, enums = {}) {
  const name = type.endsWith('?') ? type.slice(0, -1) : type;
  if (!scalarTypes.has(name) && !own(enums, name)) throw Error(`Неизвестный тип ${type}`);
}

export function matchesType(value, type, enums = {}) {
  if (type.endsWith('?')) return value == null || matchesType(value, type.slice(0, -1), enums);
  if (own(enums, type)) return typeof value === 'string' && enums[type].some(variant => value === `${type}.${variant}`);
  const raw = value?.expr ?? value;
  switch (type) {
    case 'String': case 'Asset': return typeof value === 'string';
    case 'Bool': case 'Boolean': return typeof value === 'boolean';
    case 'Number': case 'Float': return typeof value === 'number' && Number.isFinite(value);
    case 'Int': return Number.isSafeInteger(value);
    case 'Color': return typeof raw === 'string' && /^#(?:[\da-f]{3}|[\da-f]{6}|[\da-f]{8})$/i.test(raw);
    case 'Length': return typeof raw === 'number' && Number.isFinite(raw) && raw >= 0 || typeof raw === 'string' && /^(?:\d+(?:\.\d+)?)(?:px|%)$/.test(raw);
    case 'Duration': return typeof raw === 'string' && /^\d+(?:\.\d+)?ms$/.test(raw);
    default: return false;
  }
}

// Expansion retains source origins. Repeated runtime identities include the
// loop site and typed model key, never the item's transient array index.
export function expandStructure(nodes, props, state, environment = {}, options = {}) {
  let count = 0;
  function walk(list, env, prefix = '', depth = 0) {
    if (depth > 64) throw Error('Превышен предел глубины разметки');
    const output = [];
    for (const [index, node] of list.entries()) {
      if (++count > 16384) throw Error('Превышен предел размера разметки');
      if (node.type === 'If') {
        const condition = evaluate(node.condition, props, state, [], false, env);
        if (typeof condition !== 'boolean') throw Error('if ожидает Bool');
        output.push(...walk(condition ? node.children : node.elseChildren ?? [], env, prefix, depth + 1));
        continue;
      }
      if (node.type === 'For') {
        if(Object.hasOwn(env.locals??{},node.item))throw Error(`Вложенный for повторяет имя ${node.item}`);
        const collection = evaluate(node.collection, props, state, [], false, env);
        if (!Array.isArray(collection)) throw Error('for ожидает коллекцию');
        if (!collection.length) output.push(...walk(node.emptyChildren ?? [], env, prefix, depth + 1));
        const keys = new Set();
        for (const item of collection) {
          const local = {...env, locals: {...env.locals, [node.item]: item}};
          const key = evaluate(node.key, props, state, [], false, local);
          if (!(typeof key === 'string' || typeof key === 'number' && Number.isFinite(key))) throw Error('Ключ for должен быть строкой или конечным числом');
          const identity = JSON.stringify(key);
          if (keys.has(identity)) throw Error(`Повторный ключ for: ${identity}`);
          keys.add(identity);
          output.push(...walk(node.children, local, `${prefix}@${node.start ?? index}:${identity}/`, depth + 1));
        }
        continue;
      }
      const values = options.evaluateProps === false ? node.props : evaluateProperties(node, props, state, env);
      const eventArgs = Object.fromEntries(Object.entries(node.eventArgs ?? {}).map(([event, args]) => [event, args.map(value => evaluate(value, props, state, [], false, env))]));
      const result = {...node, props: {...values}, eventArgs, eventArgExpressions: node.eventArgs ?? {}, environment: env};
      if(options.trackOrigins){
        const sources={...selectedPropertySources(node.matches,props,state,env),...node.propertySources};
        const raw={...Object.fromEntries((node.forward??[]).map(key=>[key,{expr:`props.${key}`}])) ,...selectedProperties(node.matches,props,state,env),...node.props,...Object.fromEntries(Object.entries(node.bindings??{}).map(([key,path])=>[key,{expr:path}]))};
        result.propertyOrigins=Object.fromEntries(Object.entries(raw).map(([key,value])=>[key,propertyOrigins(value,sources[key]?{...sources[key],label:`${node.type} · экземпляр`}:node.source,props,options.propertySources)]));
      }
      if(node.type==='Slider'&&typeof values.value==='number'){
        const path=node.props.value?.expr;
        result.normalizedRange=!!node.bindings?.value||typeof path==='string'&&(path.startsWith('state.')||Object.hasOwn(env.locals??{},path.split('.')[0]));
      }
      if (prefix) result.props.key = `${prefix}${values.key ?? node.start ?? index}`;
      result.children = options.recursive === false || typeof options.recursive === 'function' && !options.recursive(node) ? node.children : walk(node.children ?? [], env, prefix, depth + 1);
      output.push(result);
    }
    return output;
  }
  return walk(nodes, environment);
}
