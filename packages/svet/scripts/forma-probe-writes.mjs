import { writeFile, rename, open } from 'node:fs/promises';
export const writeModes = ['direct', 'atomic', 'chunked'];
/** Writes only the caller-owned temporary probe file. */
export async function writeProbeSource(file, source, mode) {
  if (mode === 'direct') return writeFile(file, source);
  if (mode === 'atomic') {
    const next = file + '.next';
    await writeFile(next, source);
    return rename(next, file);
  }
  if (mode !== 'chunked') throw Error('Unknown probe write mode');
  const handle = await open(file, 'w');
  try {
    const split = Math.floor(source.length / 2);
    await handle.writeFile(source.slice(0, split));
    await new Promise(resolve => setTimeout(resolve, 20));
    await handle.writeFile(source.slice(split));
  } finally {
    await handle.close();
  }
}
