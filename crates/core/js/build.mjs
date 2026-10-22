// Builds one IIFE per library into the output directory. The script engine compiles each to
// QuickJS bytecode once and loads it into a context only when a script first uses it.
import { build } from 'esbuild';
import { readdirSync } from 'node:fs';

const outdir = process.argv[2] ?? 'dist';
const entries = readdirSync('libs').filter((f) => f.endsWith('.js'));
await build({
  entryPoints: entries.map((f) => `libs/${f}`),
  bundle: true,
  minify: true,
  format: 'iife',
  platform: 'neutral',
  // Node entry points, like Bruno's rollup nodeResolve({ browser: false }); crypto-js probes
  // require('crypto') inside a try, which fails harmlessly in the sandbox.
  mainFields: ['module', 'main'],
  external: ['crypto'],
  target: 'es2020',
  legalComments: 'none',
  logLevel: 'warning',
  outdir
});
