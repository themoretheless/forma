import { beforeEach, expect, it } from 'vitest';
import {
  batch,
  computed,
  createWasmCoreInstance,
  effect,
  getGraphTelemetry,
  getWasmCore,
  resetReactiveSystem,
  signal,
  untrack,
  withWasmCore,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());

it.each(['signal', 'computed'] as const)('forwards foreign %s and releases both graphs', kind => {
  const own = getWasmCore(),
    other = createWasmCoreInstance(),
    source = signal(1);
  const value = kind === 'computed' ? computed(() => source() * 2) : source;
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(value());
    }),
  );
  source.set(2);
  expect(seen).toEqual(kind === 'computed' ? [2, 4] : [1, 2]);
  observer.dispose();
  value.dispose();
  source.dispose();
  for (const core of [own, other])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('drops switched dependencies and protects pooled observer lifetimes', () => {
  const other = createWasmCoreInstance(),
    source = signal(1);
  const enabled = withWasmCore(other, () => signal(true));
  let runs = 0;
  const observer = withWasmCore(other, () =>
    effect(() => {
      runs++;
      if (enabled()) source();
    }),
  );
  enabled.set(false);
  source.set(2);
  expect(runs).toBe(2);
  expect(getGraphTelemetry().nodes).toBe(1);
  enabled.set(true);
  source.set(3);
  expect(runs).toBe(4);
  observer.dispose();
  let replacementRuns = 0;
  const replacement = withWasmCore(other, () =>
    effect(() => {
      enabled();
      replacementRuns++;
    }),
  );
  observer.dispose();
  source.set(4);
  expect(replacementRuns).toBe(1);
  enabled.set(false);
  expect(replacementRuns).toBe(2);
  replacement.dispose();
  enabled.dispose();
  source.dispose();
  expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
});

it('batches source writes and respects untracked foreign reads', () => {
  const other = createWasmCoreInstance(),
    source = signal(0),
    ignored = signal(0),
    seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(source());
      untrack(() => ignored());
    }),
  );
  batch(() => {
    source.set(1);
    source.set(2);
  });
  ignored.set(1);
  expect(seen).toEqual([0, 2]);
  observer.dispose();
  source.dispose();
  ignored.dispose();
});

it.each(['source', 'target'] as const)(
  'retires bridges on %s reset without touching reused slots',
  side => {
    const own = getWasmCore(),
      other = createWasmCoreInstance(),
      source = signal(0);
    let runs = 0;
    const observer = withWasmCore(other, () =>
      effect(() => {
        source();
        runs++;
      }),
    );
    withWasmCore(side === 'source' ? own : other, () => resetReactiveSystem());
    const replacement = withWasmCore(side === 'source' ? own : other, () => signal(100));
    source.set(1);
    expect(runs).toBe(1);
    expect(replacement()).toBe(100);
    observer.dispose();
    source.dispose();
    replacement.dispose();
    for (const core of [own, other])
      expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
  },
);

it('retires forwarding when source is disposed', () => {
  const own = getWasmCore(),
    other = createWasmCoreInstance(),
    source = signal(0);
  const local = withWasmCore(other, () => signal(0));
  let runs = 0;
  const observer = withWasmCore(other, () =>
    effect(() => {
      source();
      local();
      runs++;
    }),
  );
  source.dispose();
  local.set(1);
  expect(runs).toBe(2);
  observer.dispose();
  local.dispose();
  for (const core of [own, other])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('preserves foreign computed equality through target computed', () => {
  const other = createWasmCoreInstance(),
    source = signal(1),
    parity = computed(() => source() % 2);
  const doubled = withWasmCore(other, () => computed(() => parity() * 2));
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(doubled());
    }),
  );
  source.set(3);
  expect(seen).toEqual([2]);
  source.set(4);
  expect(seen).toEqual([2, 0]);
  observer.dispose();
  doubled.dispose();
  parity.dispose();
  source.dispose();
});

it.each(['signal', 'computed'] as const)(
  'honors forced equality invalidation for foreign %s',
  kind => {
    const other = createWasmCoreInstance(),
      value = { count: 0 };
    const source = signal(value, { equals: () => false });
    const read = kind === 'computed' ? computed(() => source(), { equals: () => false }) : source;
    let runs = 0;
    const observer = withWasmCore(other, () =>
      effect(() => {
        read();
        runs++;
      }),
    );
    value.count++;
    source.set(value);
    expect(runs).toBe(2);
    observer.dispose();
    read.dispose();
    source.dispose();
  },
);

it('rolls back forwarding graphs when cross-core computed construction encounters a cycle', () => {
  const own = getWasmCore(),
    other = createWasmCoreInstance();
  let left!: ReturnType<typeof computed<number>>, right!: ReturnType<typeof computed<number>>;
  left = computed(() => right());
  right = withWasmCore(other, () => computed(() => left()));
  expect(() =>
    withWasmCore(other, () =>
      effect(() => {
        left();
      }),
    ),
  ).toThrow(/cyclic/i);
  left.dispose();
  right.dispose();
  for (const core of [own, other])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('coalesces two foreign sources from one batch into one target notification', () => {
  const other = createWasmCoreInstance(),
    left = signal(0),
    right = signal(0),
    seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(left() + right());
    }),
  );
  try {
    batch(() => {
      left.set(1);
      right.set(2);
    });
    expect(seen).toEqual([0, 3]);
  } finally {
    observer.dispose();
    left.dispose();
    right.dispose();
  }
});

it('combines nested batches and foreign local writes with bridge notifications', () => {
  const own = getWasmCore(),
    other = createWasmCoreInstance(),
    left = signal(0),
    right = signal(0);
  const local = withWasmCore(other, () => signal(0));
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(left() + right() + local());
    }),
  );
  batch(() => {
    local.set(4);
    withWasmCore(other, () =>
      batch(() => {
        left.set(1);
        right.set(2);
      }),
    );
    expect(seen).toEqual([0]);
  });
  expect(seen).toEqual([0, 7]);
  observer.dispose();
  left.dispose();
  right.dispose();
  local.dispose();
  for (const core of [own, other])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('drains foreign batches after a throwing body and preserves its error', () => {
  const other = createWasmCoreInstance(),
    left = signal(0),
    right = signal(0),
    seen: number[] = [],
    failure = Error('batch body');
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(left() + right());
    }),
  );
  expect(() =>
    batch(() => {
      left.set(1);
      right.set(2);
      throw failure;
    }),
  ).toThrow(failure);
  expect(seen).toEqual([0, 3]);
  left.set(2);
  expect(seen).toEqual([0, 3, 4]);
  observer.dispose();
  left.dispose();
  right.dispose();
});

it('ignores a retired held batch when the target resets before delivery', () => {
  const other = createWasmCoreInstance(),
    source = signal(0);
  const local = withWasmCore(other, () => signal(0));
  let runs = 0;
  const observer = withWasmCore(other, () =>
    effect(() => {
      source();
      local();
      runs++;
    }),
  );
  batch(() => {
    local.set(1);
    withWasmCore(other, () => resetReactiveSystem());
    source.set(1);
  });
  expect(runs).toBe(1);
  const replacement = withWasmCore(other, () => signal(7));
  let latest = 0;
  const next = withWasmCore(other, () =>
    effect(() => {
      latest = replacement();
    }),
  );
  replacement.set(8);
  expect(latest).toBe(8);
  next.dispose();
  replacement.dispose();
  observer.dispose();
  local.dispose();
  source.dispose();
});

it('releases every target after one target observer throws during batch delivery', () => {
  const b = createWasmCoreInstance(),
    c = createWasmCoreInstance(),
    source = signal(0),
    seen: number[] = [];
  const first = withWasmCore(b, () =>
    effect(() => {
      if (source() === 1) throw Error('target');
    }),
  );
  const second = withWasmCore(c, () =>
    effect(() => {
      seen.push(source());
    }),
  );
  expect(() => batch(() => source.set(1))).toThrow('target');
  expect(seen).toEqual([0, 1]);
  source.set(2);
  expect(seen).toEqual([0, 1, 2]);
  first.dispose();
  second.dispose();
  source.dispose();
});

it('reads current target computed values inside a source batch', () => {
  const other = createWasmCoreInstance(),
    source = signal(1);
  const doubled = withWasmCore(other, () => computed(() => source() * 2));
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(doubled());
    }),
  );
  try {
    batch(() => {
      source.set(2);
      expect(doubled()).toBe(4);
      expect(seen).toEqual([2]);
    });
    expect(seen).toEqual([2, 4]);
  } finally {
    observer.dispose();
    doubled.dispose();
    source.dispose();
  }
});

it('refreshes foreign boundaries below cached local computed chains within batch', () => {
  const other = createWasmCoreInstance(),
    source = signal(1);
  const first = withWasmCore(other, () => computed(() => source() * 2));
  const second = withWasmCore(other, () => computed(() => first() + 1));
  const third = withWasmCore(other, () => computed(() => second() * 3));
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(third());
    }),
  );
  batch(() => {
    source.set(2);
    expect(third.peek()).toBe(15);
    expect(seen).toEqual([9]);
  });
  expect(seen).toEqual([9, 15]);
  observer.dispose();
  third.dispose();
  second.dispose();
  first.dispose();
  source.dispose();
});

it('batches replacement graph writes after a held target resets inside the same batch', () => {
  const other = createWasmCoreInstance(),
    old = withWasmCore(other, () => signal(0));
  let replacement!: ReturnType<typeof signal<number>>, observer!: ReturnType<typeof effect>;
  const seen: number[] = [];
  batch(() => {
    old.set(1);
    withWasmCore(other, () => resetReactiveSystem());
    replacement = withWasmCore(other, () => signal(2));
    observer = withWasmCore(other, () =>
      effect(() => {
        seen.push(replacement());
      }),
    );
    replacement.set(3);
    replacement.set(4);
    expect(seen).toEqual([2]);
  });
  expect(seen).toEqual([2, 4]);
  observer.dispose();
  replacement.dispose();
  old.dispose();
});

it.each([false, true])('bounds cross-core feedback and permits recovery (batch=%s)', batched => {
  const other = createWasmCoreInstance(),
    left = signal(0),
    right = withWasmCore(other, () => signal(0));
  let enabled = false;
  const a = effect(() => {
    const value = left();
    if (enabled) right.set(value + 1);
  });
  const b = withWasmCore(other, () =>
    effect(() => {
      const value = right();
      if (enabled) left.set(value + 1);
    }),
  );
  try {
    enabled = true;
    expect(() => (batched ? batch(() => left.set(1)) : left.set(1))).toThrow(/stabiliz/i);
    enabled = false;
    batch(() => {
      left.set(1000);
      right.set(1000);
    });
    expect([left(), right()]).toEqual([1000, 1000]);
  } finally {
    enabled = false;
    a.dispose();
    b.dispose();
    left.dispose();
    right.dispose();
  }
});

it.each([
  { batched: false, reverse: false },
  { batched: false, reverse: true },
  { batched: true, reverse: false },
  { batched: true, reverse: true },
])('delivers a three-core diamond without glitches ($batched/$reverse)', ({ batched, reverse }) => {
  const own = getWasmCore(),
    middle = createWasmCoreInstance(),
    target = createWasmCoreInstance();
  const source = signal(1),
    left = computed(() => source() * 2);
  const right = withWasmCore(middle, () => computed(() => source() * 3));
  const seen: number[] = [];
  const observer = withWasmCore(target, () =>
    effect(() => {
      seen.push(reverse ? right() + left() : left() + right());
    }),
  );
  try {
    if (batched) batch(() => source.set(2));
    else source.set(2);
    expect(seen).toEqual([5, 10]);
  } finally {
    observer.dispose();
    right.dispose();
    left.dispose();
    source.dispose();
  }
  for (const core of [own, middle, target])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('coalesces three-core diamond updates originating inside a running effect', () => {
  const own = getWasmCore(),
    middle = createWasmCoreInstance(),
    target = createWasmCoreInstance();
  const trigger = signal(0),
    source = signal(1),
    left = computed(() => source() * 2);
  const right = withWasmCore(middle, () => computed(() => source() * 3)),
    seen: number[] = [];
  const observer = withWasmCore(target, () =>
    effect(() => {
      seen.push(right() + left());
    }),
  );
  const writer = effect(() => {
    const value = trigger();
    if (value) source.set(value);
  });
  try {
    trigger.set(2);
    expect(seen).toEqual([5, 10]);
  } finally {
    writer.dispose();
    observer.dispose();
    right.dispose();
    left.dispose();
    source.dispose();
    trigger.dispose();
  }
  for (const core of [own, middle, target])
    expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
});

it('coalesces three-core diamond updates from effect initialization', () => {
  const middle = createWasmCoreInstance(),
    target = createWasmCoreInstance();
  const source = signal(1),
    left = computed(() => source() * 2);
  const right = withWasmCore(middle, () => computed(() => source() * 3)),
    seen: number[] = [];
  const observer = withWasmCore(target, () =>
    effect(() => {
      seen.push(right() + left());
    }),
  );
  const writer = effect(() => {
    source.set(2);
  });
  try {
    expect(seen).toEqual([5, 10]);
  } finally {
    writer.dispose();
    observer.dispose();
    right.dispose();
    left.dispose();
    source.dispose();
  }
});

it.each([false, true])(
  'recovers foreign computed failures observed by target effect (initialFailure=%s)',
  initialFailure => {
    const own = getWasmCore(),
      other = createWasmCoreInstance(),
      source = signal(initialFailure ? 1 : 0);
    const value = computed(() => {
      if (source() % 2) throw Error('foreign failure');
      return 7;
    });
    const seen: Array<number | string> = [];
    const observer = withWasmCore(other, () =>
      effect(() => {
        try {
          seen.push(value());
        } catch {
          seen.push('failed');
        }
      }),
    );
    try {
      if (!initialFailure) {
        source.set(1);
        expect(seen).toEqual([7, 'failed']);
      }
      source.set(2);
      expect(seen).toEqual(initialFailure ? ['failed', 7] : [7, 'failed', 7]);
      source.set(3);
      expect(seen.at(-1)).toBe('failed');
      source.set(4);
      expect(seen.at(-1)).toBe(7);
    } finally {
      observer.dispose();
      value.dispose();
      source.dispose();
    }
    for (const core of [own, other])
      expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
  },
);

it('recovers a failed foreign computed through a cached target computed', () => {
  const other = createWasmCoreInstance(),
    source = signal(0);
  const value = computed(() => {
    if (source() % 2) throw Error('foreign failure');
    return 7;
  });
  const local = withWasmCore(other, () => computed(() => value() * 2));
  const seen: Array<number | string> = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      try {
        seen.push(local());
      } catch {
        seen.push('failed');
      }
    }),
  );
  try {
    source.set(1);
    source.set(2);
    expect(seen).toEqual([14, 'failed', 14]);
  } finally {
    observer.dispose();
    local.dispose();
    value.dispose();
    source.dispose();
  }
});

it('propagates an uncaught target failure and recovers on the next foreign write', () => {
  const other = createWasmCoreInstance(),
    source = signal(0);
  const value = computed(() => {
    if (source() % 2) throw Error('foreign failure');
    return 7;
  });
  const seen: number[] = [];
  const observer = withWasmCore(other, () =>
    effect(() => {
      seen.push(value());
    }),
  );
  try {
    expect(() => source.set(1)).toThrow('foreign failure');
    source.set(2);
    expect(seen).toEqual([7, 7]);
  } finally {
    observer.dispose();
    value.dispose();
    source.dispose();
  }
});
