import * as uuid from 'uuid';
// Bruno safe mode exposes exactly these (bruno-js/src/sandbox/quickjs/shims/lib/uuid.js:4).
const names = ['version', 'parse', 'stringify', 'v1', 'v1ToV6', 'v3', 'v4', 'v5', 'v6', 'v6ToV1', 'v7', 'validate'];
const out = {};
for (const name of names) out[name] = uuid[name];
globalThis.__plvLibs.uuid = out;
