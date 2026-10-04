/**
 * Live WB-002 harness: a real `scrin-server` (rendezvous + relay + gateway),
 * a real Windows host (`scrin-engine` example `controller_cli host`, built
 * with `--features win`: DXGI capture + H.264 + SendInput) and a same-origin
 * front door for the built SPA.
 *
 * Why a front door: the SPA fetches `/v1/info` and `/v1/resolve` from the
 * server, and scrin-server sends no CORS headers. The front door serves
 * `apps/web/dist` and proxies `/v1/*` (HTTP and the `/v1/ws` upgrade) on ONE
 * TCP port P, while scrin-server listens for WebTransport on UDP port P. The
 * browser therefore reaches `https://127.0.0.1:P/v1/gw` (UDP) and
 * `ws://127.0.0.1:P/v1/ws` (TCP) from the origin `http://127.0.0.1:P`, exactly
 * as it would against one public server.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { createReadStream, existsSync, mkdtempSync, rmSync, statSync } from 'node:fs';
import {
  createServer,
  request,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';
import { connect, createServer as createTcpServer } from 'node:net';
import { tmpdir } from 'node:os';
import { extname, join, normalize, resolve, sep } from 'node:path';

const repo = resolve(import.meta.dirname, '../../../..');
const dist = resolve(repo, 'apps/web/dist');
const exe = process.platform === 'win32' ? '.exe' : '';
const serverBin = resolve(repo, `target/debug/scrin-server${exe}`);
const hostBin = resolve(repo, `target/debug/examples/controller_cli${exe}`);

const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff2': 'font/woff2',
};

interface HostLine {
  id: string;
  code: string;
  online: boolean;
}

export interface Harness {
  /** Origin of the SPA + proxied API, e.g. `http://127.0.0.1:51234`. */
  web: string;
  /** Plain-HTTP API listener of scrin-server. */
  api: string;
  /** Latest `HOST id=… code=… online=…` line of the host. */
  host(): HostLine | null;
  /** WebSocket upgrades proxied to `/v1/ws` so far. */
  wsUpgrades(): number;
  /** Tail of every process's output (for failure reports). */
  logs(): string;
  stop(): Promise<void>;
}

async function freePort(): Promise<number> {
  return new Promise((ok, fail) => {
    const s = createTcpServer();
    s.once('error', fail);
    s.listen(0, '127.0.0.1', () => {
      const a = s.address();
      const port = typeof a === 'object' && a ? a.port : 0;
      s.close(() => {
        ok(port);
      });
    });
  });
}

function collect(name: string, p: ChildProcess, sink: string[], onLine?: (l: string) => void) {
  let buf = '';
  const take = (chunk: Buffer) => {
    buf += chunk.toString('utf8');
    for (let i = buf.indexOf('\n'); i >= 0; i = buf.indexOf('\n')) {
      const line = buf.slice(0, i).replace(/\r$/, '');
      buf = buf.slice(i + 1);
      onLine?.(line);
      // Never keep the one-time code in logs that may end up in a report.
      sink.push(`[${name}] ${line.replace(/code=\S+/, 'code=[redacted]')}`);
      if (sink.length > 400) sink.shift();
    }
  };
  p.stdout?.on('data', take);
  p.stderr?.on('data', take);
}

async function waitFor<T>(what: string, ms: number, probe: () => T | null | Promise<T | null>) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await probe();
    if (v !== null) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

function serveStatic(req: IncomingMessage, res: ServerResponse) {
  const url = new URL(req.url ?? '/', 'http://x');
  let path = normalize(join(dist, decodeURIComponent(url.pathname)));
  if (!path.startsWith(dist + sep) && path !== dist) {
    res.writeHead(403).end();
    return;
  }
  if (!existsSync(path) || statSync(path).isDirectory()) path = join(dist, 'index.html');
  res.writeHead(200, {
    'content-type': TYPES[extname(path)] ?? 'application/octet-stream',
    'cache-control': 'no-store',
  });
  createReadStream(path).pipe(res);
}

function frontDoor(apiPort: number, counters: { ws: number }): Server {
  const srv = createServer((req, res) => {
    if (req.url === '/__harness') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ wsUpgrades: counters.ws }));
      return;
    }
    if (!req.url?.startsWith('/v1/')) {
      serveStatic(req, res);
      return;
    }
    const up = request(
      { host: '127.0.0.1', port: apiPort, path: req.url, method: req.method, headers: req.headers },
      (r) => {
        res.writeHead(r.statusCode ?? 502, r.headers);
        r.pipe(res);
      },
    );
    up.on('error', () => res.writeHead(502).end());
    req.pipe(up);
  });
  srv.on('upgrade', (req: IncomingMessage, socket, head: Buffer) => {
    if (!req.url?.startsWith('/v1/')) {
      socket.destroy();
      return;
    }
    if (req.url.startsWith('/v1/ws')) counters.ws += 1;
    const up = connect(apiPort, '127.0.0.1', () => {
      const lines = [`${req.method ?? 'GET'} ${req.url ?? '/'} HTTP/1.1`];
      for (let i = 0; i < req.rawHeaders.length; i += 2) {
        lines.push(`${req.rawHeaders[i] ?? ''}: ${req.rawHeaders[i + 1] ?? ''}`);
      }
      up.write(`${lines.join('\r\n')}\r\n\r\n`);
      if (head.length > 0) up.write(head);
      up.pipe(socket);
      socket.pipe(up);
    });
    const kill = () => {
      up.destroy();
      socket.destroy();
    };
    up.on('error', kill);
    socket.on('error', kill);
  });
  return srv;
}

const HOST_LINE = /^HOST id=(\d{9}) code=(\S+) online=(true|false)/;

export async function startHarness(): Promise<Harness> {
  for (const [bin, how] of [
    [serverBin, 'cargo build -p scrin-server'],
    [hostBin, 'cargo build -p scrin-engine --example controller_cli --features win'],
  ] as const) {
    if (!existsSync(bin)) throw new Error(`missing ${bin}; build it first: ${how}`);
  }
  if (!existsSync(join(dist, 'index.html')))
    throw new Error('apps/web/dist is missing: build the web app first (see e2e/live/README.md)');

  const logs: string[] = [];
  const procs: ChildProcess[] = [];
  const data = mkdtempSync(join(tmpdir(), 'scrin-live-'));
  const counters = { ws: 0 };
  const apiPort = await freePort();
  const webPort = await freePort();
  const api = `http://127.0.0.1:${apiPort}`;

  const server = spawn(
    serverBin,
    [
      '--tls',
      'none',
      '--listen',
      `127.0.0.1:${apiPort}`,
      // WebTransport (UDP) on the front door's port number: same origin host:port.
      '--wt-listen',
      `127.0.0.1:${webPort}`,
      '--relay-url',
      api,
      '--data-dir',
      join(data, 'server'),
      '--log',
      'info,scrin_server=debug',
    ],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  );
  procs.push(server);
  collect('server', server, logs);

  const door = frontDoor(apiPort, counters);
  await new Promise<void>((ok) => door.listen(webPort, '127.0.0.1', ok));

  const stop = async () => {
    for (const p of procs) if (p.exitCode === null) p.kill();
    await new Promise<void>((ok) => door.close(() => ok()));
    await Promise.all(
      procs.map((p) =>
        p.exitCode === null ? new Promise((ok) => p.once('exit', ok)) : Promise.resolve(),
      ),
    );
    rmSync(data, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  };

  try {
    await waitFor('scrin-server /health', 30_000, async () => {
      if (server.exitCode !== null) throw new Error(`scrin-server exited (${server.exitCode})`);
      try {
        return (await fetch(`${api}/health`)).ok ? true : null;
      } catch {
        return null;
      }
    });

    let host: HostLine | null = null;
    const hostProc = spawn(hostBin, ['host', '--secs', '900'], {
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        SCRIN_SERVER: api,
        SCRIN_DATA: join(data, 'host'),
        RUST_LOG: 'info,iroh=warn,noq=warn',
      },
    });
    procs.push(hostProc);
    collect('host', hostProc, logs, (l) => {
      const m = HOST_LINE.exec(l);
      if (m?.[1] && m[2]) host = { id: m[1], code: m[2], online: m[3] === 'true' };
    });
    await waitFor('the host to register', 60_000, () => {
      if (hostProc.exitCode !== null) throw new Error(`host exited (${hostProc.exitCode})`);
      const h: HostLine | null = host;
      return h?.online ? h : null;
    });

    return {
      web: `http://127.0.0.1:${webPort}`,
      api,
      host: () => host,
      wsUpgrades: () => counters.ws,
      logs: () => logs.join('\n'),
      stop,
    };
  } catch (e) {
    const tail = logs.slice(-40).join('\n');
    await stop();
    throw new Error(`${e instanceof Error ? e.message : String(e)}\n${tail}`, { cause: e });
  }
}
