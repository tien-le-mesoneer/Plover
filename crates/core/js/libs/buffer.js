import { Buffer } from 'buffer';
import btoa from 'btoa';
import atob from 'atob';
// The Node builds of btoa/atob call the global Buffer.
globalThis.Buffer = Buffer;
globalThis.__plvLibs.buffer = { Buffer, btoa, atob };
