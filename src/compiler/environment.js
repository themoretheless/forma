// @ts-check
// What the expressions of one compile are evaluated against, and the properties of the entry
// document itself.

import {
  evaluate,
  selectedProperties,
  selectedPropertySources,
  validateContract,
} from './component-semantics.js';
import { own } from '../shared/object.js';

/**
 * @param {object} document the parsed entry document
 * @param {object} rawRootProps properties of the root node before evaluation; a numeric size
 *   there is the viewport
 * @param {{ enums: object, tokens: object, i18n: object, styles: object }} declarations
 * @param {{ viewport?: { width?: number, height?: number }, locale?: string }} options
 */
export function evaluationEnvironment(
  document,
  rawRootProps,
  { enums, tokens, i18n, styles },
  options,
) {
  const vpW =
    typeof rawRootProps.width === 'number'
      ? rawRootProps.width
      : Number(options.viewport?.width ?? 360);
  const vpH =
    typeof rawRootProps.height === 'number'
      ? rawRootProps.height
      : Number(options.viewport?.height ?? 220);
  const viewport = { width: vpW, height: vpH, compact: vpW < 640 };
  const stateDefaults = Object.fromEntries(
    Object.entries(document.stateDefinitions ?? {})
      .filter(([, d]) => d.default !== undefined)
      .map(([k, d]) => [k, d.default]),
  );
  const environment = {
    enums,
    tokens,
    i18n,
    styles,
    stateDefaults,
    locale: options.locale ?? null,
    viewport,
  };
  return { viewport, stateDefaults, environment };
}

/**
 * Resolves the properties of the entry document, checks them against its contract and records
 * where each value comes from.
 *
 * @param {object} document
 * @param {string} entry path of the document
 * @param {object} state
 * @param {object} environment as returned by `evaluationEnvironment`
 * @param {object | undefined} properties values passed by the caller; they win over defaults
 */
export function documentProperties(document, entry, state, environment, properties) {
  const documentProps = { ...document.defaults, ...properties };
  for (const [key, definition] of Object.entries(document.propDefinitions ?? {}))
    if (definition.required && !own(documentProps, key))
      throw Error(`${document.name}: обязательное свойство ${key} не задано`);
  Object.assign(
    documentProps,
    selectedProperties(document.matches, documentProps, state, environment),
    properties,
  );
  validateContract(
    document.propDefinitions,
    Object.fromEntries(
      Object.entries(documentProps).map(([key, value]) => [
        key,
        evaluate(value, documentProps, state, [key], false, environment),
      ]),
    ),
    environment.enums,
    document.name,
  );
  if (document.forward?.length)
    throw Error('forward применяется к дочернему элементу, не к объявлению component');
  const documentSources = {
    ...Object.fromEntries(
      Object.entries(document.defaultRanges ?? {}).map(([key, range]) => [
        key,
        { file: entry, ...range, label: document.name },
      ]),
    ),
    ...selectedPropertySources(document.matches, documentProps, state, environment),
  };
  return { documentProps, documentSources };
}
