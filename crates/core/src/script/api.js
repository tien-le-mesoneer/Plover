// The `bru`, `req` and `res` objects scripts use, built from the job input (__brs.input)
// and reported back through __brs.resultParts. Semantics follow Bruno's bruno-js:
// src/bru.js, src/bruno-request.js, src/bruno-response.js and src/utils.js
// (createResponseParser).
(function () {
  'use strict';
  const g = globalThis;
  const brs = g.__brs;
  const input = brs.input;
  const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
  const unsupported = (name) => () => {
    throw new Error(name + ' is not supported by bru-rs yet');
  };

  // ---- variables -----------------------------------------------------------------------
  const SCOPES = ['runtime', 'environment', 'collection', 'folder', 'request'];
  const vars = input.vars || {};
  for (const scope of SCOPES) vars[scope] = vars[scope] || {};
  const info = input.info || {};

  // Values go as JSON so `{{obj.path}}` can walk objects (lodash `get`, like Bruno).
  const layersJson = () => {
    const layers = {};
    for (const scope of SCOPES) layers[scope] = vars[scope];
    layers.process_env = info.processEnv || {};
    return JSON.stringify(layers);
  };
  // bru.js `interpolate`: strings are interpolated, objects via their JSON, anything else as is.
  const interpolate = (value) => {
    if (!value) return value;
    if (typeof value === 'object') return JSON.parse(g.__brsInterpolate(JSON.stringify(value), layersJson()));
    if (typeof value !== 'string') return value;
    return g.__brsInterpolate(value, layersJson());
  };
  brs.interpolate = interpolate;

  const VARIABLE_NAME = /^[\w-.]*$/;
  const assertValidVariableName = (key) => {
    if (!VARIABLE_NAME.test(key)) {
      throw new Error(
        'Variable name: "' + key + '" contains invalid characters!' +
          ' Names must only contain alpha-numeric characters, "-", "_", "."'
      );
    }
  };
  const setIn = (scope, kind) => (key, value) => {
    if (!key) throw new Error('Creating a ' + kind + ' without specifying a name is not allowed.');
    assertValidVariableName(key);
    vars[scope][key] = value;
  };
  const readIn = (scope) => (key) => interpolate(vars[scope][key]);
  const clear = (scope) => () => {
    for (const key of Object.keys(vars[scope])) delete vars[scope][key];
  };

  const flow = { skipRequest: false, stopExecution: false, nextRequestSet: false, nextRequest: null };
  const setNextRequest = (name) => {
    flow.nextRequestSet = true;
    flow.nextRequest = name === null || name === undefined ? null : String(name);
  };

  const bru = {
    interpolate,
    cwd: () => info.collectionPath,
    getEnvName: () => (info.envName === null ? undefined : info.envName),
    getCollectionName: () => info.collectionName,
    // bru.js:209-214: the raw value, undefined when unset.
    getProcessEnv: (key) => (info.processEnv && hasOwn(info.processEnv, key) ? info.processEnv[key] : undefined),
    isSafeMode: () => true,

    hasEnvVar: (key) => hasOwn(vars.environment, key),
    getEnvVar: readIn('environment'),
    setEnvVar: setIn('environment', 'env variable'),
    deleteEnvVar: (key) => {
      delete vars.environment[key];
    },
    getAllEnvVars: () => Object.assign({}, vars.environment),
    deleteAllEnvVars: clear('environment'),

    hasVar: (key) => hasOwn(vars.runtime, key),
    getVar: (key) => {
      assertValidVariableName(key);
      return interpolate(vars.runtime[key]);
    },
    setVar: setIn('runtime', 'variable'),
    deleteVar: (key) => {
      delete vars.runtime[key];
    },
    deleteAllVars: clear('runtime'),
    getAllVars: () => Object.assign({}, vars.runtime),

    hasCollectionVar: (key) => hasOwn(vars.collection, key),
    getCollectionVar: readIn('collection'),
    setCollectionVar: setIn('collection', 'variable'),
    deleteCollectionVar: (key) => {
      delete vars.collection[key];
    },
    deleteAllCollectionVars: clear('collection'),
    getAllCollectionVars: () => Object.assign({}, vars.collection),
    getFolderVar: readIn('folder'),
    getRequestVar: readIn('request'),

    setNextRequest,
    runner: {
      skipRequest: () => {
        flow.skipRequest = true;
      },
      stopExecution: () => {
        flow.stopExecution = true;
      },
      setNextRequest
    },
    sleep: (ms) => new Promise((resolve) => g.setTimeout(resolve, ms)),

    runRequest: unsupported('bru.runRequest'),
    hasGlobalEnvVar: unsupported('bru.hasGlobalEnvVar'),
    getGlobalEnvVar: unsupported('bru.getGlobalEnvVar'),
    setGlobalEnvVar: unsupported('bru.setGlobalEnvVar'),
    deleteGlobalEnvVar: unsupported('bru.deleteGlobalEnvVar'),
    getAllGlobalEnvVars: unsupported('bru.getAllGlobalEnvVars'),
    deleteAllGlobalEnvVars: unsupported('bru.deleteAllGlobalEnvVars'),
    getOauth2CredentialVar: unsupported('bru.getOauth2CredentialVar'),
    resetOauth2Credential: unsupported('bru.resetOauth2Credential')
  };
  // ---- HTTP from scripts: bru.sendRequest (shims/bru.js:315-333, 512-536) and axios
  // (shims/lib/axios.js). Requests go through the run's session (TLS options, body cap) via
  // the host; like Bruno, they don't share the request's cookie jar.
  const AXIOS_ENCODE = (v) =>
    encodeURIComponent(v)
      .replace(/%3A/gi, ':')
      .replace(/%24/g, '$')
      .replace(/%2C/gi, ',')
      .replace(/%20/g, '+')
      .replace(/%5B/gi, '[')
      .replace(/%5D/gi, ']');
  const isBytes = (v) => v !== null && typeof v === 'object' && (v instanceof ArrayBuffer || ArrayBuffer.isView(v));
  const headerPairs = (headers) => {
    const out = [];
    if (!headers || typeof headers !== 'object') return out;
    for (const name of Object.keys(headers)) {
      const value = headers[name];
      if (value === undefined || value === null || value === false) continue;
      out.push([name, Array.isArray(value) ? value.join(', ') : String(value)]);
    }
    return out;
  };
  const headersObject = (pairs) => {
    const out = {};
    for (const [name, value] of pairs) {
      const key = name.toLowerCase();
      if (key === 'set-cookie') (out[key] = out[key] || []).push(value);
      else out[key] = hasOwn(out, key) ? out[key] + ', ' + value : value;
    }
    return out;
  };
  const buildUrl = (c) => {
    let url = String(c.url === undefined || c.url === null ? '' : c.url);
    if (c.baseURL && !/^([a-z][a-z\d+\-.]*:)?\/\//i.test(url)) {
      url = String(c.baseURL).replace(/\/+$/, '') + '/' + url.replace(/^\/+/, '');
    }
    if (c.params && typeof c.params === 'object') {
      const parts = [];
      for (const key of Object.keys(c.params)) {
        const value = c.params[key];
        if (value === undefined || value === null) continue;
        for (const v of Array.isArray(value) ? value : [value]) {
          const text = v instanceof Date ? v.toISOString() : typeof v === 'object' ? JSON.stringify(v) : String(v);
          parts.push(AXIOS_ENCODE(key) + (Array.isArray(value) ? '[]' : '') + '=' + AXIOS_ENCODE(text));
        }
      }
      if (parts.length) {
        const hash = url.indexOf('#');
        if (hash !== -1) url = url.slice(0, hash);
        url += (url.indexOf('?') === -1 ? '?' : '&') + parts.join('&');
      }
    }
    return url;
  };
  const toHttp = (config, source) => {
    const c = typeof config === 'string' ? { url: config } : Object.assign({}, config);
    const headers = headerPairs(c.headers);
    const hasHeader = (name) => headers.some(([k]) => k.toLowerCase() === name);
    let body = null;
    const data = c.data;
    if (data !== undefined && data !== null) {
      if (typeof data === 'string') body = { text: data };
      else if (isBytes(data)) body = { base64: g.Buffer.from(data).toString('base64') };
      else {
        body = { text: JSON.stringify(data) };
        if (!hasHeader('content-type')) headers.push(['Content-Type', 'application/json']);
      }
    }
    if (c.auth && typeof c.auth === 'object') {
      const user = c.auth.username || '';
      const pass = c.auth.password || '';
      for (let i = headers.length - 1; i >= 0; i--) if (headers[i][0].toLowerCase() === 'authorization') headers.splice(i, 1);
      headers.push(['Authorization', 'Basic ' + g.Buffer.from(user + ':' + pass).toString('base64')]);
    }
    return {
      source,
      method: String(c.method || 'get').toUpperCase(),
      url: buildUrl(c),
      headers,
      body,
      timeoutMs: typeof c.timeout === 'number' && c.timeout > 0 ? c.timeout : null,
      maxRedirects: typeof c.maxRedirects === 'number' ? c.maxRedirects : null
    };
  };
  const parseData = (text) => {
    if (typeof text !== 'string' || text === '') return text;
    try {
      return JSON.parse(text);
    } catch (_) {
      return text;
    }
  };
  const statusOk = (config, status) => {
    const validate = config && typeof config === 'object' && hasOwn(config, 'validateStatus')
      ? config.validateStatus
      : (s) => s >= 200 && s < 300;
    return !validate || validate(status);
  };
  const badStatusCode = (status) => (status >= 500 ? 'ERR_BAD_RESPONSE' : 'ERR_BAD_REQUEST');
  // One HTTP exchange through the host: { req, cfg, r } with r = { ok, status, statusText,
  // headers, text, durationMs } or { ok: false, error: { message, code } }.
  const exchange = (config, source) => {
    const req = toHttp(config, source);
    const r = JSON.parse(g.__brsHost('http', JSON.stringify(req)));
    const cfg = {
      url: req.url,
      method: req.method.toLowerCase(),
      headers: headersObject(req.headers),
      data: config && typeof config === 'object' ? config.data : undefined,
      timeout: req.timeoutMs || 0
    };
    return { req, cfg, r };
  };

  const sendRequestNow = (config) => {
    const { req, cfg, r } = exchange(config, 'sendRequest');
    if (!r.ok) {
      throw { message: r.error.message, name: 'AxiosError', code: r.error.code, config: cfg, stack: 'AxiosError: ' + r.error.message };
    }
    const response = {
      status: r.status,
      statusText: r.statusText,
      headers: headersObject(r.headers),
      data: parseData(r.text),
      config: cfg,
      request: { method: req.method, url: req.url, headers: cfg.headers },
      responseTime: r.durationMs
    };
    if (!statusOk(config, r.status)) {
      const message = 'Request failed with status code ' + r.status;
      throw { message, name: 'AxiosError', code: badStatusCode(r.status), config: cfg, status: r.status, stack: 'AxiosError: ' + message };
    }
    return response;
  };
  bru.sendRequest = async (requestConfig, callback) => {
    if (!callback) return sendRequestNow(requestConfig);
    let response;
    try {
      response = sendRequestNow(requestConfig);
    } catch (error) {
      const errObj = JSON.parse(JSON.stringify(error));
      if (errObj && errObj.response && typeof errObj.response.status === 'number') errObj.status = errObj.response.status;
      await callback(errObj, null);
      return undefined;
    }
    await callback(null, response);
    return response;
  };

  const axiosRequest = (config) =>
    Promise.resolve().then(() => {
      const { cfg, r } = exchange(config, 'axios');
      if (!r.ok) throw { message: r.error.message, code: r.error.code, isAxiosError: true, config: cfg };
      const response = { status: r.status, headers: headersObject(r.headers), data: parseData(r.text) };
      if (!statusOk(config, r.status)) {
        throw {
          message: 'Request failed with status code ' + r.status,
          code: badStatusCode(r.status),
          isAxiosError: true,
          response: { status: r.status, statusText: r.statusText, headers: response.headers, data: response.data },
          config: cfg
        };
      }
      return response;
    });
  const axios = (urlOrConfig, config) =>
    axiosRequest(typeof urlOrConfig === 'string' ? Object.assign({}, config, { url: urlOrConfig }) : urlOrConfig);
  for (const method of ['get', 'delete']) {
    axios[method] = (url, config) => axiosRequest(Object.assign({}, config, { url, method }));
  }
  for (const method of ['post', 'put', 'patch']) {
    axios[method] = (url, data, config) => axiosRequest(Object.assign({}, config, { url, method, data }));
  }
  brs.addModule('axios', () => axios, 'axios');

  // ---- path: Bruno safe mode offers only resolve (shims/lib/path.js); relative paths start
  // at the collection root (Bruno: the host process's working directory).
  const resolvePath = (...segments) => {
    let resolved = '';
    let absolute = false;
    for (let i = segments.length - 1; i >= -1 && !absolute; i--) {
      const part = i >= 0 ? segments[i] : info.collectionPath || '/';
      if (typeof part !== 'string') throw new TypeError('The "paths[' + i + ']" argument must be of type string');
      if (!part) continue;
      resolved = part + '/' + resolved;
      absolute = part.charAt(0) === '/';
    }
    const out = [];
    for (const piece of resolved.split('/')) {
      if (!piece || piece === '.') continue;
      if (piece === '..') out.pop();
      else out.push(piece);
    }
    return '/' + out.join('/');
  };
  brs.addModule('path', () => ({ resolve: resolvePath }), 'path');

  // ---- cookies: bru.cookies (cookie-list.js) over the session's cookie jar ---------------
  const cookieHost = (op, args) => JSON.parse(g.__brsHost('cookies.' + op, JSON.stringify(args || {})));
  // tough-cookie's JSON for jar methods; the property list uses cookie-list.js's shape.
  const expiresText = (c) => (typeof c.expiresMs === 'number' ? new Date(c.expiresMs).toISOString() : undefined);
  const jarCookie = (c) => {
    if (!c) return c;
    const out = { key: c.key, value: c.value, domain: c.domain, path: c.path };
    if (expiresText(c)) out.expires = expiresText(c);
    if (c.secure) out.secure = true;
    out.hostOnly = c.hostOnly;
    out.httpOnly = c.httpOnly;
    return out;
  };
  const listCookie = (c) => ({
    key: c.key,
    value: c.value,
    domain: c.domain,
    path: c.path,
    secure: c.secure,
    httpOnly: c.httpOnly,
    expires: expiresText(c) || 'Infinity'
  });
  const withCallback = async (fn, callback) => {
    if (!callback) return fn();
    let result;
    try {
      result = fn();
    } catch (err) {
      await callback(err, null);
      return undefined;
    }
    await callback(null, result);
    return undefined;
  };
  const jar = () => ({
    getCookie: (url, name, cb) =>
      withCallback(() => {
        const c = cookieHost('get', { url: interpolate(url), name });
        return c ? jarCookie(c) : null;
      }, cb),
    getCookies: (url, cb) => withCallback(() => cookieHost('list', { url: interpolate(url) }).map(jarCookie), cb),
    setCookie: (url, nameOrCookie, valueOrCallback, maybeCallback) => {
      const isCookie = typeof nameOrCookie === 'object' && nameOrCookie !== null;
      const cb = isCookie
        ? typeof valueOrCallback === 'function'
          ? valueOrCallback
          : undefined
        : typeof maybeCallback === 'function'
          ? maybeCallback
          : typeof valueOrCallback === 'function'
            ? valueOrCallback
            : undefined;
      const cookie = isCookie ? nameOrCookie : { key: nameOrCookie, value: typeof valueOrCallback === 'string' ? valueOrCallback : '' };
      return withCallback(() => {
        cookieHost('set', { url: interpolate(url), cookie });
      }, cb);
    },
    setCookies: (url, cookies, cb) =>
      withCallback(() => {
        for (const cookie of cookies || []) cookieHost('set', { url: interpolate(url), cookie });
      }, cb),
    clear: (cb) =>
      withCallback(() => {
        cookieHost('clear');
      }, cb),
    deleteCookies: (url, cb) =>
      withCallback(() => {
        cookieHost('delete', { url: interpolate(url) });
      }, cb),
    deleteCookie: (url, name, cb) =>
      withCallback(() => {
        cookieHost('delete', { url: interpolate(url), name });
      }, cb),
    hasCookie: (url, name, cb) => withCallback(() => cookieHost('get', { url: interpolate(url), name }) !== null, cb)
  });
  const currentUrl = () => interpolate(reqState.url);
  const items = () => {
    const url = currentUrl();
    return url ? cookieHost('list', { url }).map(listCookie) : [];
  };
  const cookieList = {
    get: (name) => {
      const c = items().find((i) => i.key === name);
      return c ? c.value : undefined;
    },
    one: (name) => items().find((i) => i.key === name),
    all: () => items(),
    idx: (index) => items()[index],
    count: () => items().length,
    indexOf: (item) => {
      const key = typeof item === 'string' ? item : item && item.key;
      return items().findIndex((i) => i.key === key);
    },
    has: (name, value) => items().some((i) => i.key === name && (value === undefined || i.value === value)),
    find: (fn) => items().find(fn),
    filter: (fn) => items().filter(fn),
    each: (fn) => items().forEach(fn),
    map: (fn) => items().map(fn),
    reduce: (fn, ...rest) => items().reduce(fn, ...rest),
    toObject: () => {
      const out = {};
      for (const i of items()) out[i.key] = i.value;
      return out;
    },
    toString: () => items().map((i) => i.key + '=' + i.value).join('; '),
    toJSON: () => items(),
    add: (cookie, cb) => cookieList.upsert(cookie, cb),
    upsert: (cookie, cb) => {
      if (!cookie || typeof cookie !== 'object') {
        const error = new Error('cookieObj must be a non-null object');
        if (cb) return cb(error);
        return Promise.reject(error);
      }
      return jar().setCookie(reqState.url, cookie, cb);
    },
    remove: (name, cb) => jar().deleteCookie(reqState.url, name, cb),
    delete: (name, cb) => cookieList.remove(name, cb),
    clear: (cb) => jar().deleteCookies(reqState.url, cb),
    jar
  };
  Object.defineProperty(bru, 'cookies', { value: cookieList, enumerable: false });
  g.bru = bru;

  brs.resultParts.vars = () => vars;
  brs.resultParts.skipRequest = () => flow.skipRequest;
  brs.resultParts.stopExecution = () => flow.stopExecution;
  brs.resultParts.nextRequestSet = () => flow.nextRequestSet;
  brs.resultParts.nextRequest = () => flow.nextRequest;

  // ---- req -----------------------------------------------------------------------------
  const r = input.req || { url: '', method: 'GET', headers: [], body: { mode: 'none' } };
  const reqState = {
    url: r.url,
    method: r.method,
    headers: {},
    // The body as Bruno keeps it in `req.data`: a string, or an object for form bodies.
    data: undefined,
    mode: (r.body && r.body.mode) || 'none',
    timeout: r.timeoutMs === null ? undefined : r.timeoutMs,
    maxRedirects: r.maxRedirects === null ? undefined : r.maxRedirects
  };
  for (const [name, value] of r.headers || []) reqState.headers[name] = value;
  if (reqState.mode === 'text') reqState.data = r.body.text;
  if (reqState.mode === 'form') {
    reqState.data = {};
    for (const [name, value] of r.body.fields) reqState.data[name] = value;
  }

  const mediaType = (headers) => {
    const contentType = (headers && (headers['Content-Type'] || headers['content-type'])) || '';
    return String(contentType).split(';')[0].trim().toLowerCase();
  };
  const hasJSONContentType = (headers) => mediaType(headers).includes('json');
  const safeParseJSON = (text) => {
    try {
      return JSON.parse(text);
    } catch (_) {
      return text;
    }
  };
  const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
  const parseUrl = (url) => {
    const m = /^(?:([a-zA-Z][\w+.-]*:)?\/\/)?([^/?#]*)([^?#]*)(\?[^#]*)?/.exec(String(url || ''));
    return { host: m ? m[2] : '', pathname: (m && m[3]) || '/', queryString: m && m[4] ? m[4].slice(1) : '' };
  };

  const req = {
    getUrl: () => reqState.url,
    setUrl: (url) => {
      reqState.url = url;
    },
    getHost: () => parseUrl(reqState.url).host,
    getPath: () => parseUrl(reqState.url).pathname,
    getQueryString: () => parseUrl(reqState.url).queryString,
    getMethod: () => reqState.method,
    setMethod: (method) => {
      reqState.method = method;
    },
    getAuthMode: () => {
      const auth = reqState.headers['Authorization'];
      if (typeof auth === 'string' && auth.startsWith('Bearer')) return 'bearer';
      if (typeof auth === 'string' && auth.startsWith('Basic')) return 'basic';
      return 'none';
    },
    getHeaders: () => reqState.headers,
    setHeaders: (headers) => {
      reqState.headers = headers;
    },
    getHeader: (name) => reqState.headers[name],
    setHeader: (name, value) => {
      reqState.headers[name] = value;
    },
    deleteHeader: (name) => {
      delete reqState.headers[name];
    },
    deleteHeaders: (names) => names.forEach((name) => delete reqState.headers[name]),
    hasJSONContentType,
    getBody: (options = {}) => {
      if (options.raw) return reqState.data;
      return hasJSONContentType(reqState.headers) && typeof reqState.data === 'string'
        ? safeParseJSON(reqState.data)
        : reqState.data;
    },
    setBody: (data, options = {}) => {
      if (!options.raw && hasJSONContentType(reqState.headers) && isObject(data)) {
        reqState.data = JSON.stringify(data);
      } else {
        reqState.data = data;
      }
      reqState.mode = reqState.mode === 'form' && isObject(reqState.data) ? 'form' : 'text';
    },
    getTimeout: () => reqState.timeout,
    setTimeout: (ms) => {
      reqState.timeout = ms;
    },
    setMaxRedirects: (n) => {
      reqState.maxRedirects = n;
    },
    getName: () => info.requestName,
    onFail: unsupported('req.onFail'),
    disableParsingResponseJson: unsupported('req.disableParsingResponseJson'),
    getPathParams: unsupported('req.getPathParams'),
    getTags: unsupported('req.getTags'),
    getExecutionMode: unsupported('req.getExecutionMode')
  };
  for (const [prop, get] of [
    ['url', () => reqState.url],
    ['method', () => reqState.method],
    ['headers', () => reqState.headers],
    ['body', () => req.getBody()],
    ['timeout', () => reqState.timeout],
    ['name', () => info.requestName]
  ]) {
    Object.defineProperty(req, prop, { get, enumerable: true });
  }
  g.req = req;
  brs.req = req;

  brs.resultParts.req = () => {
    const headers = [];
    for (const [name, value] of Object.entries(reqState.headers || {})) {
      if (value === undefined || value === null) continue;
      headers.push([name, typeof value === 'string' ? value : String(value)]);
    }
    let body = { mode: 'none' };
    if (reqState.mode === 'multipart') body = { mode: 'multipart' };
    else if (reqState.mode === 'form' && isObject(reqState.data)) {
      body = { mode: 'form', fields: Object.entries(reqState.data).map(([k, v]) => [k, String(v)]) };
    } else if (reqState.data !== undefined && reqState.data !== null) {
      body = { mode: 'text', text: typeof reqState.data === 'string' ? reqState.data : JSON.stringify(reqState.data) };
    }
    const count = (v) => (typeof v === 'number' && isFinite(v) && v >= 0 ? Math.floor(v) : null);
    return {
      url: String(reqState.url),
      method: String(reqState.method),
      headers,
      body,
      timeoutMs: count(reqState.timeout),
      maxRedirects: count(reqState.maxRedirects)
    };
  };

  // ---- chai plugins: Bruno's custom assertions (runtime/assert-runtime.js:14-200 and
  // sandbox/quickjs/shims/test.js) plus chai-string's startWith/endWith ------------------
  brs.onLibLoad('chai', (chai) => {
    chai.use((chai, utils) => {
      const A = chai.Assertion;
      A.addProperty('json', function () {
        const obj = this._obj;
        const isJson =
          typeof obj === 'object' &&
          obj !== null &&
          (Array.isArray(obj) || Object.prototype.toString.call(obj) === '[object Object]');
        this.assert(isJson, 'expected #{this} to be JSON', 'expected #{this} not to be JSON');
      });
      const SCHEMA_VERSIONS = ['http://json-schema.org/draft-07/schema#', 'http://json-schema.org/draft-07/schema'];
      let defaultAjv = null;
      A.addMethod('jsonSchema', function (schema, ajvOptions) {
        if (schema && schema.$schema && !SCHEMA_VERSIONS.includes(schema.$schema)) {
          this.assert(
            false,
            'Unsupported JSON Schema version: "' + schema.$schema + '". Bruno currently only supports Draft-07 (http://json-schema.org/draft-07/schema#). Please update your schema to be Draft-07 compatible and remove the $schema property.',
            'Unsupported JSON Schema version: "' + schema.$schema + '".'
          );
        }
        const { Ajv, addFormats } = brs.lib('ajv');
        let ajv;
        if (ajvOptions) {
          ajv = new Ajv(Object.assign({ allErrors: true }, ajvOptions));
          addFormats(ajv);
        } else {
          if (!defaultAjv) {
            defaultAjv = new Ajv({ allErrors: true });
            addFormats(defaultAjv);
          }
          ajv = defaultAjv;
        }
        let validate;
        try {
          validate = ajv.compile(schema);
        } catch (e) {
          this.assert(false, 'JSON schema compile error: ' + e.message, 'JSON schema compile error: ' + e.message);
        }
        const isValid = validate(this._obj);
        this.assert(
          isValid,
          'expected #{this} to match JSON schema, validation errors: ' + (validate.errors ? JSON.stringify(validate.errors) : 'none'),
          'expected #{this} to not match JSON schema'
        );
      });
      A.addMethod('match', function (regex) {
        const obj = this._obj;
        const match = obj === undefined ? false : regex.test(obj);
        this.assert(match, 'expected ' + utils.inspect(obj) + ' to match ' + regex, 'expected ' + utils.inspect(obj) + ' not to match ' + regex);
      });
      const parsePath = (path) => {
        const keys = [];
        let i = 0;
        while (i < path.length) {
          if (path[i] === '.') {
            i++;
          } else if (path[i] === '[') {
            i++;
            let key = '';
            if (i < path.length && (path[i] === "'" || path[i] === '"')) {
              const quote = path[i++];
              while (i < path.length && path[i] !== quote) {
                if (path[i] === '\\' && i + 1 < path.length && path[i + 1] === quote) {
                  key += quote;
                  i += 2;
                } else key += path[i++];
              }
              i += 2;
            } else {
              while (i < path.length && path[i] !== ']') key += path[i++];
              i++;
            }
            keys.push(key);
          } else {
            let key = '';
            while (i < path.length && path[i] !== '.' && path[i] !== '[') key += path[i++];
            keys.push(key);
          }
        }
        return keys;
      };
      const nested = (obj, path) => {
        let current = obj;
        for (const key of parsePath(path)) {
          if (current === null || current === undefined || !hasOwn(Object(current), key)) return { found: false };
          current = current[key];
        }
        return { found: true, value: current };
      };
      A.addMethod('jsonBody', function (...args) {
        const obj = this._obj;
        if (args.length === 0) {
          this.assert(typeof obj === 'object' && obj !== null, 'expected ' + utils.inspect(obj) + ' to be a JSON body (object or array)', 'expected ' + utils.inspect(obj) + ' not to be a JSON body');
        } else if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
          this.assert(utils.eql(obj, args[0]), 'expected body to deeply equal ' + utils.inspect(args[0]), 'expected body to not deeply equal ' + utils.inspect(args[0]));
        } else if (args.length === 1) {
          const result = nested(obj, String(args[0]));
          this.assert(result.found, "expected body to have nested property '" + args[0] + "'", "expected body to not have nested property '" + args[0] + "'");
        } else {
          const result = nested(obj, String(args[0]));
          this.assert(
            result.found && utils.eql(result.value, args[1]),
            "expected body to have nested property '" + args[0] + "' equal to " + utils.inspect(args[1]),
            "expected body to not have nested property '" + args[0] + "' equal to " + utils.inspect(args[1])
          );
        }
      });
      // chai-string (used by Bruno's assert runtime for startsWith/endsWith).
      const stringCheck = (name, verb, test) =>
        A.addMethod(name, function (expected) {
          const actual = this._obj;
          this.assert(
            typeof actual === 'string' && typeof expected === 'string' && test(actual, expected),
            'expected ' + actual + ' to ' + verb + ' ' + expected,
            'expected ' + actual + ' not to ' + verb + ' ' + expected
          );
        });
      stringCheck('startWith', 'start with', (a, e) => a.startsWith(e));
      stringCheck('startsWith', 'start with', (a, e) => a.startsWith(e));
      stringCheck('endWith', 'end with', (a, e) => a.endsWith(e));
      stringCheck('endsWith', 'end with', (a, e) => a.endsWith(e));
    });
  });

  // ---- test() (bruno-js/src/test.js; results in completion order) -----------------------
  const testResults = [];
  const pendingTests = [];
  const errorMessage = (e, fallback) =>
    e && typeof e === 'object' && e.message !== undefined && e.message !== '' ? String(e.message) : e && typeof e !== 'object' ? String(e) : fallback;
  g.test = (description, callback) => {
    const run = (async () => {
      try {
        await callback();
        testResults.push({ name: String(description), passed: true, error: null });
      } catch (e) {
        // bruno-js/src/test.js: `error.message || 'An unexpected error occurred.'`.
        const message = e !== null && e !== undefined && e.message ? String(e.message) : 'An unexpected error occurred.';
        testResults.push({ name: String(description), passed: false, error: message });
      }
    })();
    pendingTests.push(run);
    return run;
  };
  // Like Bruno's waitForPendingTests: the phase ends when every test() has settled,
  // including tests registered by other tests.
  brs.onFinish(async () => {
    while (pendingTests.length) await Promise.all(pendingTests.splice(0));
  });
  brs.resultParts.tests = () => testResults;

  // ---- expressions --------------------------------------------------------------------
  // Bruno evaluates assert/post-response expressions with variables and bru/req/res in
  // scope (utils.js evaluateJsExpression). `with` over a null-prototype object gives the
  // same lookup; Function bodies are sloppy-mode code, so `with` is allowed.
  const scopeOf = (layers) => {
    const scope = Object.create(null);
    for (const layer of layers) Object.assign(scope, layer);
    scope.bru = bru;
    scope.req = req;
    if (brs.res) scope.res = brs.res;
    return scope;
  };
  const evaluate = (scope, code) => new Function('__brsScope', 'with (__brsScope) { return (' + code + '\n); }')(scope);
  // sandbox/quickjs/index.js executeQuickJsVm with scriptType 'template-literal'.
  const evaluateTemplateLiteral = (text, scope) => {
    if (typeof text !== 'string' || !text.length) return text;
    text = text.trim();
    if (!text.length) return text;
    if (!isNaN(Number(text))) {
      const number = Number(text);
      if (number > Number.MAX_SAFE_INTEGER) return text;
      return Number.isInteger(number) ? parseInt(text, 10) : parseFloat(text);
    }
    if (text === 'true') return true;
    if (text === 'false') return false;
    if (text === 'null') return null;
    if (text === 'undefined') return undefined;
    if ((text.startsWith('"') && text.endsWith('"')) || (text.startsWith("'") && text.endsWith("'"))) text = text.slice(1, -1);
    return evaluate(scope, '`' + text + '`');
  };

  // ---- assert block (runtime/assert-runtime.js) ---------------------------------------
  const OPERATORS = ['eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'in', 'notIn', 'contains', 'notContains', 'length', 'matches', 'notMatches', 'startsWith', 'endsWith', 'between', 'isEmpty', 'isNotEmpty', 'isNull', 'isUndefined', 'isDefined', 'isTruthy', 'isFalsy', 'isJson', 'isNumber', 'isString', 'isBoolean', 'isArray'];
  const UNARY = ['isEmpty', 'isNotEmpty', 'isNull', 'isUndefined', 'isDefined', 'isTruthy', 'isFalsy', 'isJson', 'isNumber', 'isString', 'isBoolean', 'isArray'];
  const parseAssertionOperator = (str) => {
    if (!str || typeof str !== 'string' || !str.length) return { operator: 'eq', value: str };
    const [operator, ...rest] = str.trim().split(' ');
    if (UNARY.includes(operator)) return { operator, value: '' };
    if (OPERATORS.includes(operator)) return { operator, value: rest.join(' ') };
    return { operator: 'eq', value: str };
  };
  const evaluateRhs = (rhs, operator, scope) => {
    if (UNARY.includes(operator)) return undefined;
    if (operator === 'in' || operator === 'notIn') {
      if (rhs.startsWith('[') && rhs.endsWith(']')) rhs = rhs.substring(1, rhs.length - 1);
      return rhs.split(',').map((v) => evaluateTemplateLiteral(interpolate(v.trim()), scope));
    }
    if (operator === 'between') {
      return rhs.split(',').map((v) => evaluateTemplateLiteral(interpolate(v.trim()), scope)).slice(0, 2);
    }
    if (operator === 'matches' || operator === 'notMatches') {
      if (rhs.startsWith('/') && rhs.endsWith('/')) rhs = rhs.substring(1, rhs.length - 1);
      return interpolate(rhs);
    }
    return evaluateTemplateLiteral(interpolate(rhs), scope);
  };
  const assertResults = [];
  brs.runAsserts = async () => {
    const { expect } = brs.lib('chai');
    const scope = scopeOf([vars.collection, vars.environment, vars.folder, vars.request, vars.runtime]);
    for (const [lhsExpr, rhsExpr] of input.asserts || []) {
      const { operator, value: rhsOperand } = parseAssertionOperator(rhsExpr);
      try {
        const lhs = evaluate(scope, lhsExpr);
        const rhs = evaluateRhs(rhsOperand, operator, scope);
        switch (operator) {
          case 'eq': expect(lhs).to.equal(rhs); break;
          case 'neq': expect(lhs).to.not.equal(rhs); break;
          case 'gt': expect(lhs).to.be.greaterThan(rhs); break;
          case 'gte': expect(lhs).to.be.greaterThanOrEqual(rhs); break;
          case 'lt': expect(lhs).to.be.lessThan(rhs); break;
          case 'lte': expect(lhs).to.be.lessThanOrEqual(rhs); break;
          case 'in': expect(lhs).to.be.oneOf(rhs); break;
          case 'notIn': expect(lhs).to.not.be.oneOf(rhs); break;
          case 'contains': expect(lhs).to.include(rhs); break;
          case 'notContains': expect(lhs).to.not.include(rhs); break;
          case 'length': expect(lhs).to.have.lengthOf(rhs); break;
          case 'matches': expect(lhs).to.match(new RegExp(rhs)); break;
          case 'notMatches': expect(lhs).to.not.match(new RegExp(rhs)); break;
          case 'startsWith': expect(lhs).to.startWith(rhs); break;
          case 'endsWith': expect(lhs).to.endWith(rhs); break;
          case 'between': expect(lhs).to.be.within(rhs[0], rhs[1]); break;
          case 'isEmpty': expect(lhs).to.be.empty; break;
          case 'isNotEmpty': expect(lhs).to.not.be.empty; break;
          case 'isNull': expect(lhs).to.be.null; break;
          case 'isUndefined': expect(lhs).to.be.undefined; break;
          case 'isDefined': expect(lhs).to.not.be.undefined; break;
          case 'isTruthy': expect(lhs).to.be.true; break;
          case 'isFalsy': expect(lhs).to.be.false; break;
          case 'isJson': expect(lhs).to.be.json; break;
          case 'isNumber': expect(lhs).to.be.a('number'); break;
          case 'isString': expect(lhs).to.be.a('string'); break;
          case 'isBoolean': expect(lhs).to.be.a('boolean'); break;
          case 'isArray': expect(lhs).to.be.a('array'); break;
          default: expect(lhs).to.equal(rhs);
        }
        assertResults.push({ lhs: lhsExpr, rhs: rhsExpr, operator, passed: true, error: null });
      } catch (e) {
        assertResults.push({ lhs: lhsExpr, rhs: rhsExpr, operator, passed: false, error: errorMessage(e, String(e)) });
      }
    }
  };
  brs.resultParts.asserts = () => assertResults;

  // ---- vars:post-response (runtime/vars-runtime.js) -----------------------------------
  brs.runPostVars = async () => {
    const scope = scopeOf([vars.environment, vars.runtime]);
    const errors = [];
    for (const [name, expr] of input.postVars || []) {
      try {
        const value = evaluate(scope, expr);
        if (name) bru.setVar(name, value);
      } catch (e) {
        errors.push(name + ': ' + (e && e.message !== undefined ? e.message : e));
      }
    }
    if (errors.length) {
      g.console.error(errors.length + ' error' + (errors.length === 1 ? '' : 's') + ' in post response variables: \n' + errors.join('\n'));
    }
  };

  // ---- res -----------------------------------------------------------------------------
  const utf8Length = (text) => {
    let n = 0;
    for (let i = 0; i < text.length; i++) {
      const c = text.charCodeAt(i);
      if (c < 0x80) n += 1;
      else if (c < 0x800) n += 2;
      else if (c >= 0xd800 && c <= 0xdbff) {
        n += 4;
        i++;
      } else n += 3;
    }
    return n;
  };
  // lodash `get` with a string path ("a.b[0].c", 'a["x.y"]').
  const getPath = (obj, path) => {
    const keys = [];
    String(path).replace(/[^.[\]]+|\[(?:(-?\d+)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]/g, (m, num, q, quoted) => {
      keys.push(quoted !== undefined ? quoted : num !== undefined ? num : m);
      return m;
    });
    let current = obj;
    for (const key of keys) {
      if (current === null || current === undefined) return undefined;
      current = current[key];
    }
    return current;
  };
  brs.getPath = getPath;

  const rs = input.res;
  if (rs) {
    const headers = {};
    for (const [rawName, value] of rs.headers || []) {
      const name = rawName.toLowerCase();
      if (name === 'set-cookie') (headers[name] = headers[name] || []).push(value);
      else headers[name] = hasOwn(headers, name) ? headers[name] + ', ' + value : value;
    }
    // bruno-cli/src/utils/common.js parseDataFromResponse: strip a BOM, then try JSON.
    let body = String(rs.body || '').replace(/^﻿/, '');
    try {
      body = JSON.parse(body);
    } catch (_) {}
    let bodySize = rs.size;
    let bodySet = false;

    const res = (expr) => getPath(body, expr);
    Object.assign(res, {
      status: rs.status,
      statusText: rs.statusText,
      headers,
      body,
      responseTime: rs.responseTimeMs,
      url: rs.url,
      getStatus: () => rs.status,
      getStatusText: () => rs.statusText,
      getHeader: (name) => (typeof name === 'string' ? headers[name.toLowerCase()] : null),
      getHeaders: () => headers,
      getBody: () => body,
      setBody: (data) => {
        body = data === undefined ? undefined : JSON.parse(JSON.stringify(data));
        res.body = body;
        bodySet = true;
        bodySize = body === null || body === undefined ? 0 : utf8Length(typeof body === 'string' ? body : JSON.stringify(body));
      },
      getResponseTime: () => rs.responseTimeMs,
      getUrl: () => rs.url,
      jq: unsupported('res.jq'),
      getSize: () => {
        const lines = ['HTTP/1.1 ' + rs.status + ' ' + rs.statusText];
        for (const [k, v] of Object.entries(headers)) {
          for (const one of Array.isArray(v) ? v : [v]) lines.push(k + ': ' + one);
        }
        lines.push('', '');
        const header = utf8Length(lines.join('\r\n'));
        return { header, body: bodySize, total: header + bodySize };
      }
    });
    g.res = res;
    brs.res = res;
    brs.resultParts.resBodySet = () => bodySet;
    brs.resultParts.resBody = () => (bodySet && body !== undefined ? body : null);
  }
})();
