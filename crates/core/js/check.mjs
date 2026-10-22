// Fails when dist/ is not what `npm run build` produces from the locked dependencies.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const out = mkdtempSync(join(tmpdir(), 'brs-libs-'));
execFileSync(process.execPath, ['build.mjs', out], { stdio: 'inherit' });
const built = readdirSync(out).filter((f) => f.endsWith('.js')).sort();
const committed = readdirSync('dist').filter((f) => f.endsWith('.js')).sort();
const stale =
  built.join() !== committed.join() ||
  built.some((f) => readFileSync(join(out, f), 'utf8') !== readFileSync(join('dist', f), 'utf8'));
if (stale) {
  console.error('dist/ is stale: run `npm run build` in crates/core/js');
  process.exit(1);
}
console.log(`dist/ is up to date (${built.length} libraries)`);
