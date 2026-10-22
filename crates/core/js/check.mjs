// Fails when dist/sandbox-libs.js is not what `npm run build` produces from the locked dependencies.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const out = join(mkdtempSync(join(tmpdir(), 'brs-libs-')), 'sandbox-libs.js');
execFileSync(process.execPath, ['build.mjs', out], { stdio: 'inherit' });
if (readFileSync(out, 'utf8') !== readFileSync('dist/sandbox-libs.js', 'utf8')) {
  console.error('dist/sandbox-libs.js is stale: run `npm run build` in crates/core/js');
  process.exit(1);
}
console.log('sandbox-libs.js is up to date');
