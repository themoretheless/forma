import { expect, it } from 'vitest';
import {
  createSsrStreamTransportEncoder as encoder,
  createSsrStreamTransportDecoder as decoder,
  type SsrStreamChunk,
} from '../src/engine';
const shell = (html = 'x'.repeat(256)): SsrStreamChunk => ({
  version: 1,
  sequence: 0,
  streamId: 'codec',
  payload: { type: 'shell', html },
});
const complete = (html: string): SsrStreamChunk => ({
  version: 1,
  sequence: 1,
  streamId: 'codec',
  payload: { type: 'complete', html, stateJson: '{}' },
});
it('roundtrips Unicode reference framing exactly through JSON and releases terminal state', () => {
  const encode = encoder(),
    decode = decoder(),
    first = shell('🦀<&"'.repeat(200)),
    last = complete('<main>' + first.payload.html + '</main>');
  expect(decode.decode(JSON.parse(JSON.stringify(encode.encode(first))))).toEqual(first);
  const frame = encode.encode(last);
  expect('codec' in frame).toBe(true);
  expect(JSON.stringify(frame).length).toBeLessThan(JSON.stringify(last).length);
  expect(decode.decode(JSON.parse(JSON.stringify(frame)))).toEqual(last);
  expect(() => encode.encode(last)).toThrow(/closed/);
  expect(() => decode.decode(last)).toThrow(/closed/);
});
it('falls back to full frames for unrelated or too small HTML', () => {
  for (const first of [shell('tiny'), shell()]) {
    const encode = encoder(),
      decode = decoder();
    decode.decode(encode.encode(first));
    const last = complete('unrelated');
    expect(encode.encode(last)).toEqual(last);
    expect(decode.decode(last)).toEqual(last);
  }
});
it('rejects missing, stale and cross-stream references and remains closed after failure', () => {
  const encode = encoder(),
    first = shell();
  encode.encode(first);
  const frame = encode.encode(complete(first.payload.html));
  expect('codec' in frame).toBe(true);
  if (!('codec' in frame)) throw Error('Expected reference');
  for (const changed of [
    { ...frame, referenceSequence: 99 },
    { ...frame, record: { ...frame.record, streamId: 'other' } },
    { ...frame, record: { ...frame.record, sequence: 3 } },
    { ...frame, codec: 'unknown' },
  ]) {
    const decode = decoder();
    decode.decode(first);
    expect(() => decode.decode(changed as any)).toThrow();
    expect(() => decode.decode(first)).toThrow(/closed/);
  }
  expect(() => decoder().decode(frame)).toThrow(/reference/);
});
it('bounds decoded bytes and disposes explicit abandoned codecs', () => {
  const first = shell('x'.repeat(128)),
    encode = encoder(),
    decode = decoder({ maxChunkBytes: 512 });
  encode.encode(first);
  decode.decode(first);
  const frame = encode.encode(complete(first.payload.html));
  if (!('codec' in frame)) throw Error('Expected reference');
  expect(() => decode.decode({ ...frame, prefix: '🦀'.repeat(200) })).toThrow(/maxChunkBytes/);
  const abandoned = encoder();
  abandoned.encode(first);
  abandoned.dispose();
  expect(() => abandoned.encode(complete('x'))).toThrow(/closed/);
});
it('keeps independent codec sessions isolated and rejects out-of-order plain records', () => {
  const a = encoder(),
    b = encoder();
  a.encode(shell());
  b.encode({ ...shell(), streamId: 'second' });
  expect(() => a.encode({ ...complete('x'), streamId: 'second' })).toThrow(/identity/);
  expect(() => decoder().decode(complete('x'))).toThrow(/sequence/);
});
