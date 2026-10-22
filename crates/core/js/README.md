# Script sandbox libraries

`dist/sandbox-libs.js` is the JavaScript loaded into every bru-rs script context. It mirrors
Bruno's safe mode (`packages/bruno-js/src/sandbox/bundle-libraries.js`): chai (`expect`,
`assert`), `Buffer`, `moment`, `btoa`, `atob`, `crypto-js`, `tv4`, `ajv` and `ajv-formats`, plus the
`__brsRequireObject` map that `require()` resolves against.

The bundle is committed so building bru-rs never needs Node. Rebuild it after changing
`entry.js` or the dependencies:

```sh
npm ci
npm run build   # writes dist/sandbox-libs.js
npm run check   # fails if the committed bundle is stale
```

Licenses of the bundled packages are in `dist/THIRD_PARTY_LICENSES.txt`.
