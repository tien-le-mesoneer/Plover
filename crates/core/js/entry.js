// Same globals and require map as Bruno safe mode: bruno-js/src/sandbox/bundle-libraries.js:9-37.
import { expect, assert } from 'chai';
import { Buffer } from 'buffer';
import moment from 'moment';
import btoa from 'btoa';
import atob from 'atob';
import * as cryptoJs from 'crypto-js';
import tv4 from 'tv4';
import Ajv from 'ajv';
import addFormats from 'ajv-formats';

globalThis.expect = expect;
globalThis.assert = assert;
globalThis.moment = moment;
globalThis.btoa = btoa;
globalThis.atob = atob;
globalThis.Buffer = Buffer;
globalThis.tv4 = tv4;
globalThis.Ajv = Ajv;
globalThis.addFormats = addFormats;
globalThis.__brsRequireObject = {
  chai: { expect, assert },
  moment,
  buffer: { Buffer },
  btoa,
  atob,
  'crypto-js': cryptoJs,
  tv4,
  ajv: Ajv,
  'ajv-formats': addFormats
};
