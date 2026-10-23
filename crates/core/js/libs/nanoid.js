import { nanoid } from 'nanoid';
// Bruno safe mode: `{ nanoid }`, always the default size (shims/lib/nanoid.js).
globalThis.__brsLibs.nanoid = { nanoid: () => nanoid() };
