import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once, EventEmitter} from 'node:events';
import {createRequire} from 'node:module';
import {readFileSync, realpathSync, mkdtempSync, writeFileSync, unlinkSync} from 'node:fs';
import {join, resolve, relative, isAbsolute} from 'node:path';
import {createServer} from 'node:net';
import {createPrivateKey, sign} from 'node:crypto';

import {createBrowserRegistrationRelay, RENDEZVOUS_SUBPROTOCOL, type BrowserRelaySocket} from './browserRelay.ts';

type TestSocket = BrowserRelaySocket & EventEmitter & {terminate(): void};
// ws 没有随项目安装声明，只限定本验收实际调用的 Node 边界，不引入替身。
const Ws = createRequire(import.meta.url)('ws') as {
  new(url: string, protocols: string[], options: {origin: string; ca: Buffer; rejectUnauthorized: true; servername: string; lookup(host: string, options: unknown, callback: (error: Error | null, addresses: {address: string; family: 4}[]) => void): void}): TestSocket;
  OPEN: 1; CLOSED: 3;
};

const clock = () => performance.timeOrigin + performance.now();
const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
const hostPoint = '0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299';
const phonePoint = '04bac5b11cad8f99f9c72b05cf4b9e26d244dc189f745228255a219a86d6a09eff20138bf82dc1b6d562be0fa54ab7804a3a64b6d72ccfed6b6fb6ed28bbfc117e';
function signer(host: boolean) {
  const point = Buffer.from(host ? hostPoint : phonePoint, 'hex');
  const scalar = Buffer.from(host ? 'c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721' : '57c92077664146e876760c9520d054aa93c3afb04e306705db6090308507b4d3', 'hex');
  const key = createPrivateKey({format: 'jwk', key: {kty: 'EC', crv: 'P-256', d: scalar.toString('base64url'), x: point.subarray(1, 33).toString('base64url'), y: point.subarray(33).toString('base64url')}});
  return (bytes: Uint8Array) => Uint8Array.from(sign('sha256', bytes, {key, dsaEncoding: 'ieee-p1363'}));
}
function outer(bytes = new Uint8Array()) {
  const value = new Uint8Array(24 + bytes.length);
  value.set([1, 1, 0, 1]); value.fill(9, 4, 20);
  new DataView(value.buffer).setUint32(20, bytes.length); value.set(bytes, 24);
  return value;
}
function inbox(socket: TestSocket) {
  const queue: Uint8Array[] = [];
  let waiter: ((v: Uint8Array) => void) | undefined;
  socket.on('message', (data: Buffer) => {
    const bytes = Uint8Array.from(data as Buffer);
    if (waiter) { const original = waiter; waiter = undefined; original(bytes); }
    else queue.push(bytes);
  });
  return () => queue.length ? Promise.resolve(queue.shift()!) : new Promise<Uint8Array>((resolve, reject) => {
    const timer = setTimeout(() => { waiter = undefined; reject(Error('TEST_ONLY receive deadline')); }, 2000);
    waiter = value => { clearTimeout(timer); resolve(value); };
  });
}
async function fixture() {
  const exe = process.env.CC_DESK_TEST_RELAY_EXE;
  const repo = process.env.CC_DESK_TEST_RELAY_ROOT;
  const base = process.env.CC_DESK_TEST_ARTIFACT_ROOT;
  if (!exe || !repo || !base) throw Error('explicit actual Relay executable, source root and project artifact root required');
  const project = realpathSync(resolve(import.meta.dirname, '../../..'));
  const artifact = realpathSync(base);
  const inside = relative(project, artifact);
  assert.ok(inside && !inside.startsWith('..') && !isAbsolute(inside), 'artifact root must stay inside this project');
  for (const name of ['TEMP', 'TMP', 'TMPDIR']) {
    const value = process.env[name];
    assert.ok(value, `${name} must explicitly select a project directory`);
    const scoped = relative(project, realpathSync(value));
    assert.ok(scoped && !scoped.startsWith('..') && !isAbsolute(scoped), `${name} must stay inside this project`);
  }
  const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = (listener.address() as {port: number}).port;
  await new Promise<void>((resolve, reject) => listener.close(error => error ? reject(error) : resolve()));
  const origin = `wss://foobar.com:${port}/relay`;
  const root = mkdtempSync(join(artifact, 'registration-rendezvous-'));
  const policy = join(root, 'policy.json'), now = Date.now();
  writeFileSync(policy, JSON.stringify({schema: 1, policy_id: 'TEST_ONLY_registration_rendezvous', revision: 1, issued_at_unix_ms: now, not_before_unix_ms: now, expires_unix_ms: now + 90000, listen: `127.0.0.1:${port}`, signed_wss_endpoint: origin, browser_origins: ['https://desk.invalid'], devices: [{id: '01'.repeat(16), public_key_sec1: hostPoint, role: 'Host'}, {id: '02'.repeat(16), public_key_sec1: phonePoint, role: 'Phone'}], routes: [{tunnel: '09'.repeat(16), host: '01'.repeat(16), phone: '02'.repeat(16)}]}));
  const child = spawn(exe, ['--policy', policy, '--tls-chain', join(repo, 'tests/fixtures/chain.pem'), '--tls-key', join(repo, 'tests/fixtures/end.key'), '--tls-ca', join(repo, 'tests/fixtures/root.pem')], {cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  const exited = once(child, 'exit');
  let output = '', errors = '', connections = 0;
  child.stderr.on('data', bytes => errors += bytes);
  try {
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(Error('actual Relay start deadline')), 5000);
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.stdout.on('data', bytes => { output += bytes; if (output.includes('WSS_LISTEN=')) { clearTimeout(timer); resolve(); } });
      void exited.then(() => { clearTimeout(timer); reject(Error('actual Relay startup: ' + output + errors)); }, reject);
    });
  } catch (error) { if (child.exitCode === null) child.kill(); await exited.catch(() => {}); throw error; }
  const sockets: TestSocket[] = [];
  return {
    origin, root, get connections() { return connections; },
    socket(protocol = RENDEZVOUS_SUBPROTOCOL) {
      connections++;
      const socket = new Ws(origin, protocol ? [protocol] : [], {origin: 'https://desk.invalid', ca: readFileSync(join(repo, 'tests/fixtures/root.pem')), rejectUnauthorized: true, servername: 'foobar.com', lookup: (_host, _options, callback) => callback(null, [{address: '127.0.0.1', family: 4}])});
      socket.on('error', () => {}); sockets.push(socket); return socket;
    },
    async close() {
      for (const socket of sockets) {
        if (socket.readyState === Ws.CLOSED) continue;
        const closed = new Promise<void>(resolve => socket.once('close', () => resolve()));
        const watchdog = setTimeout(() => socket.terminate(), 2000);
        socket.close(); await closed; clearTimeout(watchdog);
      }
      unlinkSync(policy);
      const watchdog = setTimeout(() => child.kill(), 4000);
      const [code] = await exited; clearTimeout(watchdog);
      writeFileSync(join(root, 'relay.stdout.log'), output); writeFileSync(join(root, 'relay.stderr.log'), errors);
      assert.equal(code, 0, 'actual Relay must retire after TEST_ONLY policy removal: ' + output + errors);
    },
  };
}
type Fixture = Awaited<ReturnType<typeof fixture>>;
async function host(f: Fixture) {
  const socket = f.socket(), next = inbox(socket);
  await once(socket, 'open'); assert.equal(socket.protocol, RENDEZVOUS_SUBPROTOCOL);
  const challenge = await next(); assert.equal(challenge.length, 66); assert.deepEqual(challenge.subarray(0, 2), new Uint8Array([1, 0]));
  const origin = new TextEncoder().encode(f.origin), prefix = new TextEncoder().encode('CC_DESK_RELAY_ADMISSION\0\x01');
  const transcript = new Uint8Array([...prefix, origin.length >> 8, origin.length & 255, ...origin, ...challenge.subarray(2), ...new Uint8Array(16).fill(1), 1]);
  socket.send(new Uint8Array([1, 1, ...new Uint8Array(16).fill(1), 1, ...signer(true)(transcript)]));
  assert.deepEqual(await next(), new Uint8Array([1, 2])); socket.send(new Uint8Array([2, 0, ...new Uint8Array(16).fill(9)]));
  const ready = next().then(bytes => assert.deepEqual(bytes, new Uint8Array([2, 1, ...new Uint8Array(16).fill(9)])));
  void ready.catch(() => {});
  return {socket, next, ready};
}
function phone(f: Fixture, options: {duration?: number; setup?: number; protocol?: string} = {}) {
  const socket = f.socket(options.protocol), started = clock(), controller = new AbortController();
  const received: Uint8Array[] = [], sent: Uint8Array[] = [];
  let signatures = 0, readyCount = 0;
  socket.on('message', (data: Buffer | ArrayBuffer) => received.push(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data)));
  const send = socket.send.bind(socket);
  socket.send = bytes => { assert.ok(bytes instanceof Uint8Array, 'production adapter sends binary frames'); sent.push(Uint8Array.from(bytes)); send(bytes); };
  const relay = createBrowserRegistrationRelay({origin: f.origin, tunnel: new Uint8Array(16).fill(9), identity: {keyId: new Uint8Array(16).fill(2), sign: async request => { signatures++; return signer(false)(request.transcript); }}, socket, mode: 'rendezvous-v1', setupDeadline: started + (options.setup ?? 5000), ioBudgetMs: 30000, registrationDeadline: started + (options.duration ?? 40000), signal: controller.signal});
  void relay.ready.then(() => readyCount++, () => {});
  return {socket, relay, started, controller, received, sent, get signatures() { return signatures; }, get readyCount() { return readyCount; }};
}

// 浏览器先登记，只在双方原 socket 收到 Ready 后启动数据；空帧跨过 30 秒，仍在原 40 秒结束。
test('RegistrationRendezvous_Keepalive_001', {timeout: 60000}, async () => {
  const f = await fixture(); let p: ReturnType<typeof phone> | undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;
  try {
    p = phone(f); const peer = p;
    const admittedEnd = clock() + 2000;
    while (!p.received.some(bytes => bytes.length === 2 && bytes[0] === 1 && bytes[1] === 2)) {
      assert.ok(clock() < admittedEnd, 'browser must reach Accepted before counterpart connects'); await delay(5);
    }
    await delay(200); assert.equal(p.readyCount, 0, 'Accepted alone must not start data');
    assert.deepEqual(p.sent.map(bytes => [...bytes.subarray(0, 2)]), [[1, 1], [2, 0]], 'only proof and Join may precede Ready');
    const h = await host(f), wire = await p.relay.ready; await h.ready;
    assert.equal(p.socket.protocol, RENDEZVOUS_SUBPROTOCOL);
    assert.ok(p.received.some(bytes => Buffer.from(bytes).equals(Buffer.from([2, 1, ...new Uint8Array(16).fill(9)]))));
    heartbeat = setInterval(() => { if (h.socket.readyState === Ws.OPEN) h.socket.send(outer()); }, 10000);
    await delay(35000); assert.equal(p.socket.readyState, Ws.OPEN); assert.equal(h.socket.readyState, Ws.OPEN);
    const request = new TextEncoder().encode('TEST_ONLY_REGISTRATION_AFTER_35_SECONDS'); await wire.write(request);
    let bytes = await h.next(), emptyFrames = 0;
    while (bytes.length === 24) { assert.deepEqual(bytes, outer()); emptyFrames++; bytes = await h.next(); }
    assert.ok(emptyFrames >= 3, 'actual Relay must forward at least three production registration heartbeats'); assert.deepEqual(bytes, outer(request));
    const reply = new TextEncoder().encode('TEST_ONLY_RENDEZVOUS_SAME_PAIR'); h.socket.send(outer(reply)); assert.deepEqual(await wire.read(16384), reply);
    await p.relay.waitClosed(); const elapsed = clock() - p.started;
    assert.ok(elapsed >= 39000 && elapsed < 45000, `original 40-second deadline, got ${elapsed}`);
    assert.equal(p.socket.readyState, Ws.CLOSED); assert.equal(p.readyCount, 1); assert.equal(f.connections, 2);
    await assert.rejects(wire.write(request));
    const result = {testOnly: true, actualRelay: true, mode: 'rendezvous-v1', verifiedCaAndServername: true, readyBeforeData: true, idleMilliseconds: 35000, totalMilliseconds: elapsed, emptyFrames, originalEndPreserved: true, connections: f.connections, signatures: peer.signatures};
    writeFileSync(join(f.root, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify(result));
  } finally { if (heartbeat !== undefined) clearInterval(heartbeat); p?.relay.close(); await f.close(); if (p) await p.relay.waitClosed(); }
});

// 对端缺席时保留原 setup 截止时间，不用注册总预算延长会合或发送空帧。
test('RegistrationRendezvous_SetupExpiry_002', {timeout: 12000}, async () => {
  const f = await fixture(); const p = phone(f, {setup: 1000});
  try {
    await assert.rejects(p.relay.ready); await p.relay.waitClosed(); const elapsed = clock() - p.started;
    assert.ok(elapsed >= 900 && elapsed < 2500, `original setup deadline, got ${elapsed}`);
    assert.equal(p.readyCount, 0); assert.equal(p.signatures, 1); assert.equal(f.connections, 1);
    assert.deepEqual(p.sent.map(bytes => [...bytes.subarray(0, 2)]), [[1, 1], [2, 0]]);
    assert.equal(p.socket.readyState, Ws.CLOSED);
  } finally { p.relay.close(); await f.close(); await p.relay.waitClosed(); }
});

// 注册原终点早于 setup 时按原终点关闭，Join 和 Accepted 均不能重启预算。
test('RegistrationRendezvous_TotalExpiry_003', {timeout: 12000}, async () => {
  const f = await fixture(); const p = phone(f, {duration: 700});
  try {
    await assert.rejects(p.relay.ready); await p.relay.waitClosed(); const elapsed = clock() - p.started;
    assert.ok(elapsed >= 600 && elapsed < 2000, `original registration deadline, got ${elapsed}`);
    assert.equal(p.readyCount, 0); assert.equal(p.signatures, 1); assert.equal(f.connections, 1);
    assert.deepEqual(p.sent.map(bytes => [...bytes.subarray(0, 2)]), [[1, 1], [2, 0]]);
    assert.equal(p.socket.readyState, Ws.CLOSED);
  } finally { p.relay.close(); await f.close(); await p.relay.waitClosed(); }
});

// 实际 Relay 走无子协议的旧握手时，新模式适配器在签名前拒绝，且不自动重连降级。
test('RegistrationRendezvous_NoDowngrade_004', {timeout: 12000}, async () => {
  const f = await fixture(); const p = phone(f, {protocol: ''});
  try {
    await assert.rejects(p.relay.ready); await p.relay.waitClosed();
    assert.equal(p.socket.protocol, ''); assert.equal(p.signatures, 0); assert.equal(p.sent.length, 0);
    assert.equal(p.readyCount, 0); assert.equal(f.connections, 1); assert.equal(p.socket.readyState, Ws.CLOSED);
    assert.ok(p.received.some(bytes => bytes.length === 66), 'must reach actual legacy Relay challenge before rejecting');
  } finally { p.relay.close(); await f.close(); await p.relay.waitClosed(); }
});

// 不支持的子协议在真实升级握手失败，不签证明、不发送 Join 或创建第二条连接。
test('RegistrationRendezvous_BadProtocol_005', {timeout: 12000}, async () => {
  const f = await fixture(); const p = phone(f, {protocol: 'cc-desk-relay-rendezvous-v999'});
  try {
    await assert.rejects(p.relay.ready); await p.relay.waitClosed();
    assert.equal(p.signatures, 0); assert.equal(p.sent.length, 0); assert.equal(p.received.length, 0);
    assert.equal(p.readyCount, 0); assert.equal(f.connections, 1); assert.equal(p.socket.readyState, Ws.CLOSED);
  } finally { p.relay.close(); await f.close(); await p.relay.waitClosed(); }
});
