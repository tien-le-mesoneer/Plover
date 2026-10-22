import { build } from 'esbuild';

const outfile = process.argv[2] ?? 'dist/sandbox-libs.js';
await build({
  entryPoints: ['entry.js'],
  bundle: true,
  minify: true,
  format: 'iife',
  platform: 'neutral',
  mainFields: ['browser', 'module', 'main'],
  target: 'es2020',
  legalComments: 'none',
  logLevel: 'warning',
  outfile
});
