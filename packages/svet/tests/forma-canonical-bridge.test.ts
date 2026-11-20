// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import {
  type CanonicalFormaDocument,
  compileFormaComponent,
  evaluateFormaExpression,
  fromCanonicalFormaAst,
  fromCanonicalFormaExpr,
  parseFormaSource,
  registerFormaFrontend,
  signal,
} from '../src/engine/index.ts';

afterEach(() => {
  registerFormaFrontend(null);
});

describe('Svet canonical .ui AST and expression bridge', () => {
  it('evaluates ?., coalesce, .. and ..= ranges, and UTF-16 string .length', () => {
    const userSig = signal<{ profile?: { name: string } } | null>({ profile: { name: 'Ada' } });
    const missingSig = signal<string | null>(null);
    const emojiSig = signal('😀');
    const i18nSig = signal<Record<string, string>>({});
    const validSig = signal(true);
    const scope = {
      state: { user: userSig, missing: missingSig, emoji: emojiSig },
      props: {},
      computedLets: {},
      tokens: {},
      i18n: i18nSig,
      isValid: validSig,
    };

    expect(evaluateFormaExpression("state.user?.profile?.name ?? 'anon'", scope)).toBe('Ada');
    userSig.set(null);
    expect(evaluateFormaExpression("state.user?.profile?.name ?? 'anon'", scope)).toBe('anon');
    expect(
      evaluateFormaExpression("coalesce(state.missing, state.user?.profile?.name, 'ok')", scope),
    ).toBe('ok');
    expect(evaluateFormaExpression('1..4', scope)).toEqual([1, 2, 3]);
    expect(evaluateFormaExpression('1..=4', scope)).toEqual([1, 2, 3, 4]);
    expect(evaluateFormaExpression('state.emoji.length', scope)).toBe(2);

    const ast = fromCanonicalFormaExpr({
      expression: {
        kind: 'binary',
        operator: '??',
        left: {
          expression: {
            kind: 'member',
            optional: true,
            object: {
              expression: {
                kind: 'member',
                optional: true,
                object: { expr: 'state.user' },
                property: 'profile',
              },
            },
            property: 'name',
          },
        },
        right: 'fallback',
      },
    });
    expect(ast.type).toBe('binary');

    userSig.dispose();
    missingSig.dispose();
    emojiSig.dispose();
    i18nSig.dispose();
    validSig.dispose();
  });

  it('converts canonical ASTs with tokens, i18n, lets, contracts, control flow, and two-way bindings', () => {
    const canonical: CanonicalFormaDocument = {
      name: 'BridgeCard',
      tokens: { 'spacing.md': 16 },
      i18n: {
        greeting: 'Hi',
        'en.greeting': 'Hi',
        en: { greeting: 'Hello' },
        ru: { greeting: 'Привет' },
      },
      styles: { pill: { radius: 8 } },
      propDefinitions: {
        limit: { type: 'Number', required: false, constraints: { range: [1, 10] } },
        code: { type: 'String', required: false, constraints: { pattern: '^[A-Z]+$' } },
      },
      defaults: { limit: 5, code: 'OK', doubled: { expr: 'state.count * 2' } },
      stateDefinitions: {
        count: { type: 'Number', default: 2 },
        query: { type: 'String', default: 'seed' },
        items: { type: 'List', default: [{ id: 'a', label: 'Alpha' }] },
      },
      lets: {
        doubled: {
          expression: {
            kind: 'binary',
            operator: '*',
            left: { expr: 'state.count' },
            right: 2,
          },
        },
      },
      shortcuts: [{ key: 'Mod+K', action: 'state.increment', args: ['count', 1] }],
      nodes: [
        {
          type: 'Column',
          apply: ['pill'],
          children: [
            {
              type: 'Text',
              props: {
                key: 'heading',
                text: {
                  interpolation: [
                    { expr: 'i18n.greeting' },
                    ':',
                    { expr: 'props.doubled' },
                    ':',
                    { expr: 'tokens.spacing.md' },
                  ],
                },
              },
            },
            {
              type: 'If',
              condition: {
                expression: {
                  kind: 'binary',
                  operator: '>',
                  left: { expr: 'props.doubled' },
                  right: 3,
                },
              },
              children: [{ type: 'Text', props: { key: 'gt', text: 'many' } }],
              elseChildren: [{ type: 'Text', props: { key: 'le', text: 'few' } }],
            },
            {
              type: 'Match',
              match: { expr: 'state.count' },
              branches: [
                {
                  pattern: { expression: { kind: 'binary', operator: '..', left: 1, right: 3 } },
                  children: [{ type: 'Text', props: { text: 'range-low' } }],
                },
                {
                  pattern: { expr: '_' },
                  children: [{ type: 'Text', props: { text: 'range-other' } }],
                },
              ],
            },
            {
              type: 'For',
              item: 'item',
              collection: { expr: 'state.items' },
              key: { expr: 'item.id' },
              children: [{ type: 'Text', props: { text: { expr: 'item.label' } } }],
              emptyChildren: [{ type: 'Text', props: { text: 'empty' } }],
            },
            {
              type: 'TextInput',
              bindings: { value: 'state.query' },
            },
          ],
        },
      ],
    };

    registerFormaFrontend(() => canonical);
    const converted = fromCanonicalFormaAst(canonical);
    expect(converted.componentName).toBe('BridgeCard');
    expect(parseFormaSource('component BridgeCard {}').componentName).toBe('BridgeCard');

    const inst = compileFormaComponent('component BridgeCard {}', { locale: 'en' });
    const host = document.createElement('div');
    const mounted = inst.mount(host);

    expect(inst.isValid()).toBe(true);
    expect(host.textContent).toContain('Hello:4:16');
    expect(host.textContent).toContain('many');
    expect(host.textContent).toContain('range-low');
    expect(host.textContent).toContain('Alpha');

    inst.locale.set('ru');
    expect(host.textContent).toContain('Привет:4:16');

    inst.state.count!.set(1);
    expect(host.textContent).toContain('Привет:2:16');
    expect(host.textContent).toContain('few');

    inst.state.count!.set(5);
    expect(host.textContent).toContain('range-other');

    inst.state.items!.set([]);
    expect(host.textContent).toContain('empty');

    inst.props.limit!.set(99);
    expect(inst.isValid()).toBe(false);
    inst.props.limit!.set(5);
    expect(inst.isValid()).toBe(true);

    const input = host.querySelector('input')!;
    expect(input.value).toBe('seed');
    input.value = 'typed';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    expect(inst.state.query!()).toBe('typed');

    mounted.unmount();
    inst.dispose();
  });
});
