import { execFileSync } from 'node:child_process';
import { root } from './versions.mjs';

// Which workspace crate may depend on which. Dependencies point inward only: the shader compiler,
// tensor contracts and CPU math know nothing of the rest, runtimes sit on them, and forma never
// reaches into the compute kernels. A new edge has to be added here on purpose.
export const ALLOWED = {
  'gpu-shader': [],
  'tensor-core': [],
  'osv-math': [],
  'gpu-runtime': ['gpu-shader'],
  'gpu-compute': ['gpu-runtime', 'gpu-shader'],
  'raster-core': ['gpu-compute', 'gpu-runtime', 'gpu-shader'],
  'compute-core': ['gpu-compute', 'gpu-runtime', 'gpu-shader', 'tensor-core'],
  'compute-mlx': ['tensor-core'],
  'compute-cuda': ['gpu-compute', 'tensor-core'],
  'osv-math-compute': [
    'osv-math',
    'compute-core',
    'gpu-compute',
    'tensor-core',
    'compute-cuda',
    'compute-mlx',
  ],
  forma: ['gpu-runtime', 'gpu-compute', 'gpu-shader', 'svet-wasm'],
  'forma-binding-example': ['forma'],
  'forma-language-example': ['forma'],
  'openscad-forma-studio': ['forma'],
};

export function violations(packages, allowed = ALLOWED) {
  const local = new Set([...Object.keys(allowed), 'svet-wasm', 'gpu-compute']);
  const found = [];
  for (const pkg of packages) {
    if (!(pkg.name in allowed)) {
      found.push(`${pkg.name}: not listed in the layering table`);
      continue;
    }
    for (const dependency of pkg.dependencies) {
      // Only path dependencies inside this repository are layering edges.
      if (!dependency.path || !local.has(dependency.name)) continue;
      if (!allowed[pkg.name].includes(dependency.name))
        found.push(`${pkg.name} -> ${dependency.name} is not an allowed dependency`);
    }
  }
  return found;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 64 << 20,
    }),
  );
  const found = violations(metadata.packages);
  if (found.length) {
    console.error(`layering: ${found.length} violation(s)\n  ${found.join('\n  ')}`);
    process.exit(1);
  }
  console.log(
    `layering: ${metadata.packages.length} workspace crates respect the dependency table`,
  );
}
