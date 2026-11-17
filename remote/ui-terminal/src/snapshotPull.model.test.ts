import {test} from 'node:test';
import assert from 'node:assert/strict';

// 拉取路线生命周期探索模型；无生产导出、无真实认证/编码/TLS/计时器证明。
class HostBudget {
  readonly views = new Set<symbol>();
  acquire() { if (this.views.size === 2) throw Error('global view budget'); const v = Symbol(); this.views.add(v); return v; }
  release(v: symbol) { assert.equal(this.views.delete(v), true); }
}
class Transfer {
  readonly context = Symbol('original-channel-target-binding-fence-stamp');
  readonly budget: HostBudget;
  readonly view: symbol;
  readonly end: number;
  readonly sizes: readonly number[];
  readonly cap: number;
  phase: 'AwaitNext'|'Sending'|'Delivered'|'Closing'|'Joined' = 'AwaitNext';
  next = 0;
  private pending: {token: symbol; through: number}|undefined;
  constructor(budget: HostBudget, sizes: readonly number[], cap: number, end: number) {
    this.budget = budget; this.view = budget.acquire(); this.sizes = sizes; this.cap = cap; this.end = end;
  }
  stop() { if (this.phase !== 'Joined') this.phase = 'Closing'; }
  tick(now: number) { if (now >= this.end) this.stop(); }
  request(context: symbol, offset: number, now: number) {
    this.tick(now);
    if (context !== this.context || this.phase !== 'AwaitNext' || offset !== this.next) {
      this.stop(); throw Error('request');
    }
    let bytes = 0, through = this.next;
    while (through < this.sizes.length && bytes + this.sizes[through]! <= this.cap) bytes += this.sizes[through++]!;
    if (through === this.next) { this.stop(); throw Error('batch cap'); }
    const token = Symbol('original-send'); this.pending = {token, through}; this.phase = 'Sending';
    return {token, bytes, through};
  }
  complete(token: symbol, now: number) {
    this.tick(now);
    if (!this.pending || this.pending.token !== token) { this.stop(); throw Error('completion'); }
    const through = this.pending.through; this.pending = undefined;
    if (this.phase === 'Closing') return;
    this.next = through;
    this.phase = this.next === this.sizes.length ? 'Delivered' : 'AwaitNext';
  }
  projectionAck(context: symbol, now: number) {
    this.tick(now);
    if (context !== this.context || this.phase !== 'Delivered') { this.stop(); throw Error('projection ack'); }
    // 模型在整个传输最后才允许原完整 ACK；它不授予输入 Window。
    this.stop();
  }
  join() {
    if (this.phase !== 'Closing' || this.pending) throw Error('original work still owned');
    this.budget.release(this.view); this.phase = 'Joined';
  }
}

test('SnapshotPullModel_GlobalBudgetSharedAcrossConnections_001', () => {
  const budget = new HostBudget(), a = new Transfer(budget, [20], 32, 1000), b = new Transfer(budget, [20], 32, 1000);
  assert.throws(() => new Transfer(budget, [20], 32, 1000), /global view budget/);
  a.stop(); a.join(); assert.equal(budget.views.size, 1);
  const c = new Transfer(budget, [20], 32, 1000); assert.equal(budget.views.size, 2);
  b.stop(); b.join(); c.stop(); c.join(); assert.equal(budget.views.size, 0);
});

test('SnapshotPullModel_OneOriginalViewAcrossBatches_002', () => {
  const budget = new HostBudget(), transfer = new Transfer(budget, [20, 20, 20], 32, 1000), original = transfer.view;
  for (let offset = 0; offset < 3; offset++) {
    const batch = transfer.request(transfer.context, offset, offset * 100);
    assert.ok(batch.bytes <= 32); assert.equal(transfer.view, original); assert.equal(budget.views.size, 1);
    assert.equal(transfer.phase, 'Sending'); transfer.complete(batch.token, offset * 100 + 10);
    assert.equal(transfer.phase, offset === 2 ? 'Delivered' : 'AwaitNext');
  }
  transfer.projectionAck(transfer.context, 400); transfer.join(); assert.equal(budget.views.size, 0);
});

test('SnapshotPullModel_ContinuationDoesNotRefreshAbsoluteDeadline_003', () => {
  const budget = new HostBudget(), transfer = new Transfer(budget, [20, 20], 32, 1000);
  const first = transfer.request(transfer.context, 0, 900); transfer.complete(first.token, 950);
  assert.throws(() => transfer.request(transfer.context, 1, 1000), /request/);
  assert.equal(transfer.end, 1000); transfer.join(); assert.equal(budget.views.size, 0);
});

test('SnapshotPullModel_RevokeDuringSendRetainsOriginalUntilJoin_004', () => {
  const budget = new HostBudget(), transfer = new Transfer(budget, [20, 20], 32, 1000);
  const batch = transfer.request(transfer.context, 0, 0); transfer.stop();
  assert.throws(() => transfer.join(), /still owned/); assert.equal(budget.views.size, 1);
  transfer.complete(batch.token, 10); assert.equal(transfer.phase, 'Closing');
  assert.equal(transfer.next, 0); transfer.join(); assert.equal(budget.views.size, 0);
});

test('SnapshotPullModel_ExpiryWhileAwaitingNextNeedsNoNewRequest_005', () => {
  const budget = new HostBudget(), transfer = new Transfer(budget, [20, 20], 32, 1000);
  const batch = transfer.request(transfer.context, 0, 0); transfer.complete(batch.token, 10);
  transfer.tick(1000); transfer.join(); assert.equal(budget.views.size, 0);
  // tick 是待映射原owner计时器的模型事件，不是真实自主回收已经通过的证据。
});

test('SnapshotPullModel_ReplayCrossContextAndOversizeClose_006', () => {
  for (const variant of ['replay', 'context', 'oversize']) {
    const budget = new HostBudget(), transfer = new Transfer(budget, variant === 'oversize' ? [33] : [20, 20], 32, 1000);
    if (variant === 'replay') { const batch = transfer.request(transfer.context, 0, 0); transfer.complete(batch.token, 1); }
    assert.throws(() => transfer.request(variant === 'context' ? Symbol() : transfer.context, 0, 2), /request|batch cap/);
    assert.equal(transfer.phase, 'Closing'); transfer.join(); assert.equal(budget.views.size, 0);
  }
});

test('SnapshotPullModel_NoEarlyDeliveredOrParallelRequest_007', () => {
  const budget = new HostBudget(), transfer = new Transfer(budget, [20, 20], 32, 1000);
  const batch = transfer.request(transfer.context, 0, 0);
  assert.throws(() => transfer.request(transfer.context, 0, 1), /request/);
  assert.equal(transfer.phase, 'Closing'); transfer.complete(batch.token, 2); transfer.join();
  const other = new Transfer(budget, [20, 20], 32, 1000);
  const first = other.request(other.context, 0, 0); other.complete(first.token, 1);
  assert.throws(() => other.projectionAck(other.context, 2), /projection ack/);
  other.join(); assert.equal(budget.views.size, 0);
});

test('SnapshotPullModel_TransferReceiptCannotStandInForConsumption_008', () => {
  let originalRpcBusy = true, batchPending = true, transportCopied = false;
  const mayRequestNext = () => originalRpcBusy && !batchPending;
  transportCopied = true; assert.equal(transportCopied, true); assert.equal(mayRequestNext(), false);
  // 只有完整认证/校验/消费批的原完成事件才能清 pending；postMessage 不行。
  batchPending = false; assert.equal(mayRequestNext(), true);
  assert.equal(originalRpcBusy, true); originalRpcBusy = false;
  assert.equal(mayRequestNext(), false);
});

// 这里只建模唯一槽位；不把 symbol 删除等同 Arc 释放或 Unknown join。
class ConnectionOwner {
  current: Transfer|undefined;
  capture(budget: HostBudget) {
    if (this.current && this.current.phase !== 'Joined') throw Error('connection already owns transfer');
    return this.current = new Transfer(budget, [20], 32, 1000);
  }
}
test('SnapshotPullModel_OneTransferSlotPerConnection_009', () => {
  const budget = new HostBudget(), a = new ConnectionOwner(), b = new ConnectionOwner();
  const first = a.capture(budget);
  assert.throws(() => a.capture(budget), /connection already owns transfer/);
  assert.equal(budget.views.size, 1);
  const other = b.capture(budget); assert.equal(budget.views.size, 2);
  first.stop(); assert.throws(() => a.capture(budget), /connection already owns transfer/);
  first.join(); const replacement = a.capture(budget); assert.equal(budget.views.size, 2);
  replacement.stop(); replacement.join(); other.stop(); other.join();
});
