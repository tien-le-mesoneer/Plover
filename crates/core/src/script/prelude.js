// Runs once per script context, after the safe-mode libraries (js/dist/sandbox-libs.js) and
// before the user's code. It installs the globals Bruno's QuickJS sandbox offers (console,
// require, timers, and the bru/req/res/test APIs from api.js) and the hooks the Rust driver
// calls: __brsStart, __brsNextTimerDelay, __brsFireTimer and __brsResult.
//
// Native helpers provided by the engine: __brsInputJson (string), __brsNow() (ms since the
// script started) and __brsInterpolate(text, layersJson).
(function () {
  'use strict';
  const g = globalThis;
  const input = JSON.parse(g.__brsInputJson || '{}');
  delete g.__brsInputJson;

  const SAFE_MODULES = 'chai, buffer, moment, btoa, atob, crypto-js, tv4, ajv, ajv-formats';

  // ---- console -------------------------------------------------------------------------
  const consoleLines = [];
  const formatValue = (v) => {
    if (typeof v === 'string') return v;
    if (v === undefined) return 'undefined';
    if (typeof v === 'function') return '[Function' + (v.name ? ': ' + v.name : '') + ']';
    if (typeof v === 'symbol' || typeof v === 'bigint') return String(v);
    if (v instanceof Error) return v.stack ? String(v.name) + ': ' + v.message + '\n' + v.stack.trimEnd() : String(v);
    try {
      const json = JSON.stringify(v);
      return json === undefined ? String(v) : json;
    } catch (_) {
      return String(v);
    }
  };
  const logAt = (level) => (...args) => {
    consoleLines.push({ level, text: args.map(formatValue).join(' ') });
  };
  g.console = {
    log: logAt('log'),
    info: logAt('info'),
    warn: logAt('warn'),
    error: logAt('error'),
    debug: logAt('debug')
  };

  // ---- libraries: loaded into this context on first use -------------------------------
  g.__brsLibs = {};
  const libHooks = {};
  const lib = (name) => {
    if (!(name in g.__brsLibs)) {
      g.__brsLoadLib(name);
      for (const hook of libHooks[name] || []) hook(g.__brsLibs[name]);
    }
    return g.__brsLibs[name];
  };
  const lazyGlobal = (prop, get) => {
    const settle = (value) =>
      Object.defineProperty(g, prop, { value, writable: true, configurable: true, enumerable: false });
    Object.defineProperty(g, prop, {
      configurable: true,
      enumerable: false,
      get() {
        const value = get();
        settle(value);
        return value;
      },
      set: settle
    });
  };
  // The globals Bruno's bundle defines (bruno-js/src/sandbox/bundle-libraries.js:20-28).
  lazyGlobal('expect', () => lib('chai').expect);
  lazyGlobal('assert', () => lib('chai').assert);
  lazyGlobal('Buffer', () => lib('buffer').Buffer);
  lazyGlobal('btoa', () => lib('buffer').btoa);
  lazyGlobal('atob', () => lib('buffer').atob);
  lazyGlobal('moment', () => lib('moment'));
  lazyGlobal('tv4', () => lib('tv4'));
  lazyGlobal('Ajv', () => lib('ajv').Ajv);
  lazyGlobal('addFormats', () => lib('ajv').addFormats);

  // ---- require (Bruno safe mode: bruno-js/src/sandbox/bundle-libraries.js:29-39) --------
  const modules = {
    chai: () => ({ expect: lib('chai').expect, assert: lib('chai').assert }),
    moment: () => lib('moment'),
    buffer: () => ({ Buffer: lib('buffer').Buffer }),
    btoa: () => lib('buffer').btoa,
    atob: () => lib('buffer').atob,
    'crypto-js': () => lib('crypto-js'),
    tv4: () => lib('tv4'),
    ajv: () => lib('ajv').Ajv,
    'ajv-formats': () => lib('ajv').addFormats
  };
  const required = {};
  g.require = (name) => {
    if (!Object.prototype.hasOwnProperty.call(modules, name)) {
      const err = new Error(
        "Cannot find module '" + name + "' — bru-rs runs scripts like Bruno safe mode; available: " + SAFE_MODULES
      );
      err.__brsForbiddenModule = String(name);
      throw err;
    }
    if (!(name in required)) required[name] = modules[name]();
    return required[name];
  };

  // ---- timers: the Rust driver sleeps until the next one is due ---------------------------
  const timers = new Map();
  let nextTimerId = 1;
  const addTimer = (fn, ms, args, repeat) => {
    if (typeof fn !== 'function') throw new TypeError('The "callback" argument must be of type function');
    const delay = Math.max(0, Number(ms) || 0);
    const id = nextTimerId++;
    timers.set(id, { fn, args, delay, repeat, due: g.__brsNow() + delay, seq: id });
    return id;
  };
  g.setTimeout = (fn, ms, ...args) => addTimer(fn, ms, args, false);
  g.setInterval = (fn, ms, ...args) => addTimer(fn, ms, args, true);
  g.clearTimeout = g.clearInterval = (id) => {
    timers.delete(id);
  };
  const nextTimer = () => {
    let best = null;
    for (const [id, t] of timers) {
      if (!best || t.due < best[1].due || (t.due === best[1].due && t.seq < best[1].seq)) best = [id, t];
    }
    return best;
  };
  g.__brsNextTimerDelay = () => {
    const next = nextTimer();
    return next ? Math.max(0, next[1].due - g.__brsNow()) : -1;
  };
  let timerError = null;
  g.__brsFireTimer = () => {
    const next = nextTimer();
    if (!next) return;
    const [id, t] = next;
    if (t.repeat) {
      t.due = g.__brsNow() + Math.max(1, t.delay);
      t.seq = nextTimerId++;
    } else {
      timers.delete(id);
    }
    try {
      t.fn(...t.args);
    } catch (e) {
      // Like an uncaught exception in a Node timer callback: it ends the script.
      if (!timerError) timerError = e;
    }
  };

  // ---- run state ----------------------------------------------------------------------
  const serializeError = (e) => {
    if (e && typeof e === 'object') {
      return {
        message: e.message !== undefined ? String(e.message) : String(e),
        name: e.name !== undefined ? String(e.name) : 'Error',
        stack: e.stack !== undefined ? String(e.stack) : null,
        forbiddenModule: e.__brsForbiddenModule || null
      };
    }
    return { message: String(e), name: 'Error', stack: null, forbiddenModule: null };
  };

  const state = { done: false, error: null };
  const finishers = [];
  const resultParts = {};
  g.__brs = {
    input,
    resultParts,
    lib,
    // Called with a library's exports right after it loads (e.g. to add chai plugins).
    onLibLoad: (name, hook) => (libHooks[name] = libHooks[name] || []).push(hook),
    // Run before the script is reported done (e.g. waiting for pending test() promises).
    onFinish: (fn) => finishers.push(fn)
  };

  g.__brsStart = (main) => {
    Promise.resolve()
      .then(main)
      .then(
        () => undefined,
        (e) => {
          state.error = serializeError(e);
        }
      )
      .then(async () => {
        for (const fn of finishers) {
          try {
            await fn();
          } catch (e) {
            if (!state.error) state.error = serializeError(e);
          }
        }
      })
      .then(() => {
        if (timerError && !state.error) state.error = serializeError(timerError);
        state.done = true;
      });
  };
  g.__brsIsDone = () => {
    if (state.done) return true;
    if (timerError) {
      if (!state.error) state.error = serializeError(timerError);
      return true;
    }
    return false;
  };

  g.__brsResult = () => {
    const parts = {};
    for (const key of Object.keys(resultParts)) {
      try {
        parts[key] = resultParts[key]();
      } catch (e) {
        parts[key] = null;
      }
    }
    return JSON.stringify({ console: consoleLines, error: state.error, ...parts });
  };
})();
