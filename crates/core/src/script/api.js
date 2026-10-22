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

  const layersJson = () => {
    const layers = {};
    for (const scope of SCOPES) {
      layers[scope] = {};
      for (const [name, value] of Object.entries(vars[scope])) {
        if (value === undefined) continue;
        layers[scope][name] = typeof value === 'string' ? value : JSON.stringify(value);
      }
    }
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

    sendRequest: unsupported('bru.sendRequest'),
    runRequest: unsupported('bru.runRequest'),
    getProcessEnv: unsupported('bru.getProcessEnv'),
    hasGlobalEnvVar: unsupported('bru.hasGlobalEnvVar'),
    getGlobalEnvVar: unsupported('bru.getGlobalEnvVar'),
    setGlobalEnvVar: unsupported('bru.setGlobalEnvVar'),
    deleteGlobalEnvVar: unsupported('bru.deleteGlobalEnvVar'),
    getAllGlobalEnvVars: unsupported('bru.getAllGlobalEnvVars'),
    deleteAllGlobalEnvVars: unsupported('bru.deleteAllGlobalEnvVars'),
    getOauth2CredentialVar: unsupported('bru.getOauth2CredentialVar'),
    resetOauth2Credential: unsupported('bru.resetOauth2Credential')
  };
  Object.defineProperty(bru, 'cookies', { get: unsupported('bru.cookies'), enumerable: false });
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
