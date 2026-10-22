# Script sandbox libraries

`dist/*.js` are the libraries bru-rs scripts can use. They mirror Bruno's safe mode
(`packages/bruno-js/src/sandbox/bundle-libraries.js`): chai (`expect`, `assert`), `Buffer`,
`moment`, `btoa`, `atob`, `crypto-js`, `tv4`, `ajv` and `ajv-formats`. Each file (one per entry in
`libs/`) registers its exports on `globalThis.__brsLibs`; the script engine compiles a library to
bytecode and loads it into a script's context only when the script first uses it
(`crates/core/src/script/prelude.js`).

The bundles are committed so building bru-rs never needs Node. Rebuild them after changing
`libs/` or the dependencies:

```sh
npm ci
npm run build   # writes dist/*.js
npm run check   # fails if the committed bundle is stale
```

Licenses of the bundled packages are in `dist/THIRD_PARTY_LICENSES.txt`.
