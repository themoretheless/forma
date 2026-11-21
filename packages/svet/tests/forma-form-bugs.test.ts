import { describe, expect, it } from 'vitest';
import { createForm, createRoot } from '../src/engine/index.ts';

describe('createForm baseline equality and owner cleanup (S-21)', () => {
  it('treats cloned object and array initialValues as clean initially and after reset()', () => {
    createRoot(dispose => {
      const form = createForm({
        initialValues: {
          profile: { name: 'Ada' },
          tags: ['ui', 'wasm'],
        },
      });

      expect(form.dirty.profile()).toBe(false);
      expect(form.dirty.tags()).toBe(false);
      expect(form.isDirty()).toBe(false);

      form.setValue('profile', { name: 'Grace' });
      expect(form.dirty.profile()).toBe(true);
      expect(form.isDirty()).toBe(true);

      form.reset();
      expect(form.dirty.profile()).toBe(false);
      expect(form.dirty.tags()).toBe(false);
      expect(form.isDirty()).toBe(false);

      dispose();
    });
  });

  it('aborts in-flight validateAsync controllers when the enclosing OwnerScope disposes', () => {
    let observedSignal: AbortSignal | undefined;
    let disposeRoot!: () => void;

    createRoot(dispose => {
      disposeRoot = dispose;
      createForm({
        initialValues: { handle: 'ada' },
        rules: {
          handle: {
            validateAsync: (_val, { signal }) => {
              observedSignal = signal;
              return new Promise(() => {});
            },
          },
        },
      });
    });

    expect(observedSignal).toBeDefined();
    expect(observedSignal?.aborted).toBe(false);
    disposeRoot();
    expect(observedSignal?.aborted).toBe(true);
  });
});
