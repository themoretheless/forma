/** Wire format shared by Node producers and browser receivers. */
export const SSR_STREAM_VERSION = 1 as const;
export type SsrStreamPayload =
  | { type: 'shell'; html: string }
  | { type: 'patch'; boundary: number; html: string }
  | { type: 'complete'; html: string; stateJson: string };
export interface SsrStreamChunk {
  version: typeof SSR_STREAM_VERSION;
  sequence: number;
  streamId: string;
  payload: SsrStreamPayload;
}

/** Opt-in application headers; transport hosts must enforce negotiation before SSR. */
export const SSR_STREAM_ACCEPT_HEADER = 'svet-stream-accept';
export const SSR_STREAM_VERSION_HEADER = 'svet-stream-version';
export const SSR_STREAM_CONTENT_TYPE = 'application/x-ndjson';
/** Strict bounded comma-separated positive integer versions, without wildcards/q-values. */
export function negotiateSsrStreamVersion(
  accepted: string | null,
): typeof SSR_STREAM_VERSION | undefined {
  if (typeof accepted !== 'string' || accepted.length > 256) return;
  const versions = accepted.split(',');
  if (
    versions.length > 16 ||
    versions.some(
      value => !/^[1-9][0-9]*$/.test(value.trim()) || !Number.isSafeInteger(Number(value.trim())),
    )
  )
    return;
  return versions.some(value => Number(value.trim()) === SSR_STREAM_VERSION)
    ? SSR_STREAM_VERSION
    : undefined;
}
export const SSR_STREAM_CODEC_HEADER = 'svet-stream-codec';
export const SSR_STREAM_HTML_REFERENCE_CODEC = 'svet-html-ref-v1' as const;
export type SsrStreamCodec = 'identity' | typeof SSR_STREAM_HTML_REFERENCE_CODEC;
/** Missing offer preserves identity; explicit incompatible/malformed offers fail. */
export function negotiateSsrStreamCodec(offered: string | null): SsrStreamCodec | undefined {
  if (offered === null) return 'identity';
  if (typeof offered !== 'string' || offered.length > 256) return;
  const codecs = offered.split(',').map(value => value.trim());
  if (codecs.length > 16 || codecs.some(value => !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value))) return;
  if (codecs.includes(SSR_STREAM_HTML_REFERENCE_CODEC)) return SSR_STREAM_HTML_REFERENCE_CODEC;
  if (codecs.includes('identity')) return 'identity';
}
/** Validate headers before reading a body or creating/mutating a DOM receiver. */
export function assertSsrStreamResponse(
  response: { ok: boolean; headers: { get(name: string): string | null } },
  options: { codec?: SsrStreamCodec } = {},
): void {
  if (!response.ok) throw Error('SSR stream HTTP request failed');
  const codec = response.headers.get(SSR_STREAM_CODEC_HEADER)?.trim() ?? 'identity';
  if (codec !== (options.codec ?? 'identity')) throw Error('Unsupported SSR stream HTTP codec');
  if (response.headers.get(SSR_STREAM_VERSION_HEADER)?.trim() !== String(SSR_STREAM_VERSION))
    throw Error('Unsupported SSR stream HTTP version');
  if (
    response.headers.get('content-type')?.split(';', 1)[0]!.trim().toLowerCase() !==
    SSR_STREAM_CONTENT_TYPE
  )
    throw Error('Invalid SSR stream HTTP content type');
}
