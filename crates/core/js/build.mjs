// Builds one IIFE per library into the output directory. The script engine compiles each to
// QuickJS bytecode once and loads it into a context only when a script first uses it.
import { build } from 'esbuild';
import { readdirSync } from 'node:fs';

const outdir = process.argv[2] ?? 'dist';
const common = {
  bundle: true,
  minify: true,
  format: 'iife',
  platform: 'neutral',
  target: 'es2020',
  legalComments: 'none',
  logLevel: 'warning',
  outdir
};
// These draw randomness from Web Crypto; the sandbox provides it as `__plvCrypto`
// (crates/core/src/script/prelude.js) instead of a `crypto` global.
const webCrypto = ['uuid.js', 'nanoid.js'];
const entries = readdirSync('libs').filter((f) => f.endsWith('.js'));
await build({
  ...common,
  entryPoints: entries.filter((f) => !webCrypto.includes(f)).map((f) => `libs/${f}`),
  // Node entry points, like Bruno's rollup nodeResolve({ browser: false }); crypto-js probes
  // require('crypto') inside a try, which fails harmlessly in the sandbox.
  mainFields: ['module', 'main'],
  external: ['crypto']
});
await build({
  ...common,
  entryPoints: webCrypto.map((f) => `libs/${f}`),
  // Browser builds: they use crypto.getRandomValues instead of Node's crypto module.
  conditions: ['browser'],
  mainFields: ['browser', 'module', 'main'],
  define: { crypto: 'globalThis.__plvCrypto' }
});
