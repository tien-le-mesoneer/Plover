// `jsonwebtoken` as Bruno safe mode exposes it (sign, verify, decode; shims/lib/jwt.js),
// following jsonwebtoken 9's sign.js / verify.js / decode.js. Signatures are computed by
// the host (`jwt.sign` / `jwt.verify`); errors cross as plain `Error`s, as in Bruno's shim.
import ms from 'ms';

// Bruno's shim rethrows `vm.newError(message)`: a plain Error whose message is enumerable.
const jwtError = (message) => {
  const error = new Error(message);
  Object.defineProperty(error, 'message', { value: message, enumerable: true, writable: true, configurable: true });
  return error;
};

const host = (op, args) => JSON.parse(globalThis.__plvHost(op, JSON.stringify(args)));

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
const utf8Bytes = (str) => {
  const bin = unescape(encodeURIComponent(str));
  const out = new Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};
const b64url = (bytes) => {
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
    out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63];
    if (i + 1 < bytes.length) out += B64[(n >> 6) & 63];
    if (i + 2 < bytes.length) out += B64[n & 63];
  }
  return out;
};
const fromB64url = (text) => {
  const clean = String(text).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
  let bin = '';
  let bits = 0;
  let acc = 0;
  for (const ch of clean) {
    const v = B64.indexOf(ch);
    if (v < 0) throw new Error('invalid base64');
    acc = (acc << 6) | v;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bin += String.fromCharCode((acc >> bits) & 255);
    }
  }
  return decodeURIComponent(escape(bin));
};

const isBuffer = (v) => v && typeof v === 'object' && typeof v.readUInt8 === 'function' && typeof v.toString === 'function';
const keyArgs = (key) =>
  isBuffer(key) ? { key: key.toString('base64'), keyEncoding: 'base64' } : { key: String(key), keyEncoding: 'utf8' };
const isPem = (key) => typeof key === 'string' && key.indexOf('-----BEGIN') !== -1;

// jsonwebtoken lib/timespan.js
const timespan = (time, iat) => {
  const timestamp = iat || Math.floor(Date.now() / 1000);
  if (typeof time === 'string') {
    const milliseconds = ms(time);
    if (typeof milliseconds === 'undefined') return undefined;
    return Math.floor(timestamp + milliseconds / 1000);
  }
  if (typeof time === 'number') return timestamp + time;
  return undefined;
};

const OPTIONS_TO_PAYLOAD = { audience: 'aud', issuer: 'iss', subject: 'sub', jwtid: 'jti' };

const settle = (callback, err, value) => {
  Promise.resolve().then(() => callback(err, value));
};

const signSync = (payload, secretOrPrivateKey, options) => {
  options = options || {};
  if (typeof payload === 'undefined') throw jwtError('payload is required');
  const isObjectPayload = typeof payload === 'object' && !isBuffer(payload);
  const header = Object.assign(
    { alg: options.algorithm || 'HS256', typ: isObjectPayload ? 'JWT' : undefined, kid: options.keyid },
    options.header
  );
  if (!secretOrPrivateKey && options.algorithm !== 'none') throw jwtError('secretOrPrivateKey must have a value');
  if (typeof options.expiresIn !== 'undefined' && !isObjectPayload) {
    throw jwtError('invalid expiresIn option for string payload');
  }
  if (isObjectPayload) payload = Object.assign({}, payload);
  const timestamp = (isObjectPayload && payload.iat) || Math.floor(Date.now() / 1000);
  if (options.noTimestamp) {
    if (isObjectPayload) delete payload.iat;
  } else if (isObjectPayload) {
    payload.iat = timestamp;
  }
  if (typeof options.notBefore !== 'undefined') {
    payload.nbf = timespan(options.notBefore, timestamp);
    if (typeof payload.nbf === 'undefined') {
      throw jwtError('"notBefore" should be a number of seconds or string representing a timespan eg: "1d", "20h", 60');
    }
  }
  if (typeof options.expiresIn !== 'undefined' && isObjectPayload) {
    payload.exp = timespan(options.expiresIn, timestamp);
    if (typeof payload.exp === 'undefined') {
      throw jwtError('"expiresIn" should be a number of seconds or string representing a timespan eg: "1d", "20h", 60');
    }
  }
  for (const key of Object.keys(OPTIONS_TO_PAYLOAD)) {
    const claim = OPTIONS_TO_PAYLOAD[key];
    if (typeof options[key] !== 'undefined') {
      if (typeof payload[claim] !== 'undefined') {
        throw jwtError('Bad "options.' + key + '" option. The payload already has an "' + claim + '" property.');
      }
      payload[claim] = options[key];
    }
  }
  const body = isObjectPayload ? JSON.stringify(payload) : isBuffer(payload) ? payload.toString() : String(payload);
  const input = b64url(utf8Bytes(JSON.stringify(header))) + '.' + b64url(utf8Bytes(body));
  if (header.alg === 'none') return input + '.';
  const signature = host('jwt.sign', Object.assign({ alg: header.alg, input }, keyArgs(secretOrPrivateKey)));
  return input + '.' + signature;
};

const decode = (token, options) => {
  options = options || {};
  if (typeof token !== 'string' || !/^[a-zA-Z0-9\-_]+?\.[a-zA-Z0-9\-_]+?\.([a-zA-Z0-9\-_]+)?$/.test(token)) return null;
  const parts = token.split('.');
  let header;
  try {
    header = JSON.parse(fromB64url(parts[0]));
  } catch (e) {
    return null;
  }
  let payload;
  try {
    payload = fromB64url(parts[1]);
  } catch (e) {
    return null;
  }
  try {
    const obj = JSON.parse(payload);
    if (obj !== null && typeof obj === 'object') payload = obj;
  } catch (e) {
    // a string payload stays a string
  }
  if (options.complete === true) return { header, payload, signature: parts[2] };
  return payload;
};

const HS = ['HS256', 'HS384', 'HS512'];
const PUBLIC = ['RS256', 'RS384', 'RS512', 'PS256', 'PS384', 'PS512', 'ES256', 'ES384', 'ES512', 'EdDSA'];

const verifySync = (token, secretOrPublicKey, options) => {
  options = Object.assign({}, options);
  const clockTimestamp = options.clockTimestamp || Math.floor(Date.now() / 1000);
  const tolerance = options.clockTolerance || 0;
  if (!token) throw jwtError('jwt must be provided');
  if (typeof token !== 'string') throw jwtError('jwt must be a string');
  const parts = token.split('.');
  if (parts.length !== 3) throw jwtError('jwt malformed');
  const decoded = decode(token, { complete: true });
  if (!decoded) throw jwtError('invalid token');
  const header = decoded.header;
  const hasSignature = parts[2].trim() !== '';
  if (!hasSignature && secretOrPublicKey) throw jwtError('jwt signature is required');
  if (hasSignature && !secretOrPublicKey) throw jwtError('secret or public key must be provided');
  const algorithms = options.algorithms || (isPem(secretOrPublicKey) ? PUBLIC : HS);
  if (!hasSignature && !options.algorithms) options.algorithms = ['none'];
  if ((options.algorithms || algorithms).indexOf(header.alg) === -1) throw jwtError('invalid algorithm');
  if (hasSignature) {
    const valid = host(
      'jwt.verify',
      Object.assign({ alg: header.alg, input: parts[0] + '.' + parts[1], signature: parts[2] }, keyArgs(secretOrPublicKey))
    );
    if (!valid) throw jwtError('invalid signature');
  }
  const payload = decoded.payload;
  if (typeof payload.nbf !== 'undefined' && !options.ignoreNotBefore) {
    if (typeof payload.nbf !== 'number') throw jwtError('invalid nbf value');
    if (payload.nbf > clockTimestamp + tolerance) throw jwtError('jwt not active');
  }
  if (typeof payload.exp !== 'undefined' && !options.ignoreExpiration) {
    if (typeof payload.exp !== 'number') throw jwtError('invalid exp value');
    if (clockTimestamp >= payload.exp + tolerance) throw jwtError('jwt expired');
  }
  if (options.audience) {
    const audiences = Array.isArray(options.audience) ? options.audience : [options.audience];
    const target = Array.isArray(payload.aud) ? payload.aud : [payload.aud];
    const match = target.some((aud) =>
      audiences.some((a) => (a instanceof RegExp ? a.test(aud) : a === aud))
    );
    if (!match) throw jwtError('jwt audience invalid. expected: ' + audiences.join(' or '));
  }
  if (options.issuer) {
    const invalid =
      (typeof options.issuer === 'string' && payload.iss !== options.issuer) ||
      (Array.isArray(options.issuer) && options.issuer.indexOf(payload.iss) === -1);
    if (invalid) throw jwtError('jwt issuer invalid. expected: ' + options.issuer);
  }
  if (options.subject && payload.sub !== options.subject) {
    throw jwtError('jwt subject invalid. expected: ' + options.subject);
  }
  if (options.jwtid && payload.jti !== options.jwtid) {
    throw jwtError('jwt jwtid invalid. expected: ' + options.jwtid);
  }
  if (options.maxAge) {
    if (typeof payload.iat !== 'number') throw jwtError('iat required when maxAge is specified');
    const maxAgeTimestamp = timespan(options.maxAge, payload.iat);
    if (typeof maxAgeTimestamp === 'undefined') {
      throw jwtError('"maxAge" should be a number of seconds or string representing a timespan eg: "1d", "20h", 60');
    }
    if (clockTimestamp >= maxAgeTimestamp + tolerance) throw jwtError('maxAge exceeded');
  }
  if (options.complete === true) return { header, payload, signature: decoded.signature };
  return payload;
};

// (payload, key, options?, callback?) and (token, key, options?, callback?), like jsonwebtoken.
const withCallback = (fn) => (first, key, options, callback) => {
  if (typeof options === 'function') {
    callback = options;
    options = {};
  }
  if (typeof callback !== 'function') return fn(first, key, options);
  let result;
  try {
    result = fn(first, key, options);
  } catch (err) {
    settle(callback, err, undefined);
    return undefined;
  }
  settle(callback, null, result);
  return undefined;
};

globalThis.__plvLibs.jsonwebtoken = {
  sign: withCallback(signSync),
  verify: withCallback(verifySync),
  decode
};
