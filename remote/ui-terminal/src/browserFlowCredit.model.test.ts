import {test} from 'node:test';
import assert from 'node:assert/strict';

// 仅为设计反例/账本模型，不导出、不接入生产，不模拟真实认证或 TLS 释放凭证。
// 原链路 RED 与未证义务见 docs/browser-flow-control-design.md。
class Ledger {
  readonly epoch = Symbol('test-only epoch');
  readonly direction = 'host-to-browser';
  readonly window: bigint;
  readonly end: number;
  emitted: bigint;
  reserved = 0n;
  acknowledged: bigint;
  closed = false;
  constructor(window: bigint, sent: bigint, acknowledged: bigint, end = 1000) {
    assert.ok(window > 0n && acknowledged >= 0n && acknowledged <= sent);
    assert.ok(sent - acknowledged <= window);
    this.window = window; this.emitted = sent; this.acknowledged = acknowledged; this.end = end;
  }
  check(now: number) {
    if (this.closed || now >= this.end) { this.closed = true; throw Error('closed'); }
  }
  reserve(bytes: bigint, now = 0) {
    this.check(now);
    if (bytes <= 0n || this.emitted + this.reserved + bytes > (1n << 64n) - 1n) {
      this.closed = true; throw Error('counter');
    }
    if (bytes > this.window - (this.emitted + this.reserved - this.acknowledged)) return false;
    this.reserved += bytes; return true;
  }
  emit(bytes: bigint, now = 0) {
    this.check(now);
    if (bytes <= 0n || bytes > this.reserved) { this.closed = true; throw Error('emission'); }
    this.reserved -= bytes; this.emitted += bytes;
  }
  acknowledge(epoch: symbol, total: bigint, now = 0, direction: string = this.direction) {
    this.check(now);
    if (epoch !== this.epoch || direction !== this.direction || total < this.acknowledged || total > this.emitted) {
      this.closed = true; throw Error('ack');
    }
    this.acknowledged = total;
  }
}

test('FlowCreditModel_BootstrapResidualCannotBeReset_001', () => {
  const ledger = new Ledger(64n, 80n, 40n);
  assert.equal(ledger.reserve(25n), false);
  assert.equal(ledger.reserve(24n), true);
  assert.equal(ledger.reserved + ledger.emitted - ledger.acknowledged, 64n);
  // 错误清零模型会多放行 40 字节；既有在途量不能因激活而消失。
  const reset = new Ledger(64n, 0n, 0n);
  assert.equal(reset.reserve(64n), true);
  assert.ok(40n + reset.reserved + reset.emitted > reset.window);
});

test('FlowCreditModel_DuplicateAcknowledgementDoesNotMintCredit_002', () => {
  const ledger = new Ledger(64n, 64n, 0n);
  ledger.acknowledge(ledger.epoch, 16n);
  for (let i = 0; i < 10000; i++) ledger.acknowledge(ledger.epoch, 16n);
  assert.equal(ledger.reserve(16n), true);
  assert.equal(ledger.reserve(1n), false);
  assert.equal(ledger.reserved + ledger.emitted - ledger.acknowledged, 64n);
  // 模型只保存累计整数，没有每次 ACK 历史数组。
  assert.equal(Object.values(ledger).some(Array.isArray), false);
});

test('FlowCreditModel_RegressionForgeryAndCrossEpochClose_003', () => {
  for (const mutation of ['regress', 'beyond', 'epoch']) {
    const ledger = new Ledger(64n, 64n, 32n);
    assert.throws(() => ledger.acknowledge(
      mutation === 'epoch' ? Symbol('other') : ledger.epoch,
      mutation === 'regress' ? 31n : mutation === 'beyond' ? 65n : 32n,
    ), /ack/);
    assert.equal(ledger.closed, true);
    assert.throws(() => ledger.reserve(1n), /closed/);
  }
});

test('FlowCreditModel_PostMessageIsNotRelease_004', () => {
  let hiddenWorkerBytes = 0n;
  const unsafe = new Ledger(64n, 0n, 0n);
  for (let i = 0; i < 100; i++) {
    assert.equal(unsafe.reserve(64n), true);
    unsafe.emit(64n); hiddenWorkerBytes += 64n;
    // 错误模型：消息仅转移到另一个队列，尚未消费，却已经回收信用。
    unsafe.acknowledge(unsafe.epoch, unsafe.emitted);
  }
  assert.ok(hiddenWorkerBytes > unsafe.window);
  const guarded = new Ledger(64n, 0n, 0n);
  assert.equal(guarded.reserve(64n), true);
  // 没有原释放收据，transfer/copy 不更新 acknowledged。
  assert.equal(guarded.reserve(1n), false);
});

test('FlowCreditModel_WasmCopyStillOwnsDownstreamBytes_005', () => {
  const residency = {ring: 0, messagePort: 0, worker: 16384, wasmTls: 0, snapshot: 0};
  const total = () => Object.values(residency).reduce((a, b) => a + b, 0);
  const before = total();
  residency.wasmTls += residency.worker; residency.worker = 0;
  assert.equal(total(), before);
  // 下游继续复制到 Snapshot 时，更不能把两份驻留误算成一次释放。
  residency.snapshot += residency.wasmTls;
  assert.equal(total(), before * 2);
});

test('FlowCreditModel_IndependentAckSlotCannotPassPartialTlsRecord_006', () => {
  const sides = [0, 1].map(() => ({dataCredit: 8 * 1024, recordTail: 9 * 1024, ackQueued: false, ackSlot: 1}));
  // 从完整初态到可达阻塞态：按字节扣额可在一条合法尺寸 record 中途耗尽。
  for (const s of sides) {
    const sent = Math.min(s.dataCredit, s.recordTail);
    s.dataCredit -= sent; s.recordTail -= sent;
    // 即使接收侧已生成进度通知，它也排在原未完成 record 后面。
    s.ackQueued = true;
    assert.equal(s.recordTail, 1024);
  }
  const step = (s: typeof sides[number]) => {
    if (s.recordTail > 0) {
      if (s.dataCredit === 0) return false;
      s.recordTail--; s.dataCredit--; return true;
    }
    if (s.ackQueued && s.ackSlot > 0) { s.ackQueued = false; return true; }
    return false;
  };
  // 这是反例而非新调度器：控制槽存在，但不能重排同一 TLS 字节流。
  assert.equal(sides.some(step), false);
  assert.ok(sides.every(s => s.ackQueued && s.ackSlot === 1));
});

test('FlowCreditModel_FragmentCountIsNotAByteBound_007', () => {
  const sameBytes = 5;
  assert.ok(sameBytes < 65536);
  const fragments = new Array(sameBytes).fill(1);
  assert.ok(fragments.length > 4);
  assert.equal(fragments.reduce((a, b) => a + b, 0), sameBytes);
});

test('FlowCreditModel_DeadlineRevokeAndCounterOverflowRemainTerminal_008', () => {
  const deadline = new Ledger(64n, 64n, 0n, 1000);
  assert.throws(() => deadline.acknowledge(deadline.epoch, 64n, 1000), /closed/);
  const revoked = new Ledger(64n, 0n, 0n); revoked.closed = true;
  assert.throws(() => revoked.reserve(1n), /closed/);
  const maximum = (1n << 64n) - 1n;
  const overflow = new Ledger(64n, maximum, maximum);
  assert.throws(() => overflow.reserve(1n), /counter/);
  assert.equal(overflow.closed, true);
});


test('FlowCreditModel_ReservedBytesCannotBeAcknowledged_009', () => {
  const ledger = new Ledger(64n, 0n, 0n);
  assert.equal(ledger.reserve(64n), true);
  ledger.emit(16n);
  assert.equal(ledger.reserved, 48n);
  assert.equal(ledger.emitted, 16n);
  ledger.acknowledge(ledger.epoch, 16n);
  assert.equal(ledger.reserve(17n), false);
  // 即使总预留达到 64，也只能确认已经由原发送完成证明的 16 字节。
  assert.throws(() => ledger.acknowledge(ledger.epoch, 17n), /ack/);
  assert.equal(ledger.closed, true);
});

test('FlowCreditModel_ControlOnlyNaivePoliciesBothFail_010', () => {
  const noControlAck = new Ledger(64n, 0n, 0n);
  for (let i = 0; i < 8; i++) {
    assert.equal(noControlAck.reserve(8n), true); noControlAck.emit(8n);
  }
  assert.equal(noControlAck.reserve(8n), false);
  // 相反，纯 ACK 每次诱发新的 ACK，不需要任何业务也能持续自激。
  let pendingAck = true, emittedAcks = 0;
  for (let i = 0; i < 100; i++) {
    assert.equal(pendingAck, true);
    pendingAck = false; emittedAcks++;
    pendingAck = true;
  }
  assert.equal(emittedAcks, 100);
  assert.equal(pendingAck, true);
  // 两个反例均未提供可用方案，不能由此把控制字节免计费。
});


test('FlowCreditModel_ReverseDirectionCannotReleaseForwardCredit_011', () => {
  const ledger = new Ledger(64n, 64n, 0n);
  assert.throws(() => ledger.acknowledge(ledger.epoch, 64n, 0, 'browser-to-host'), /ack/);
  assert.equal(ledger.closed, true);
  assert.equal(ledger.acknowledged, 0n);
});
