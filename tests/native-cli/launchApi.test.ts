import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { Channel } from '@tauri-apps/api/core';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import * as api from '@/api/tauri';
import { validateLaunchRequest } from '@/utils/nativeIdentity';
import golden from '../fixtures/native-cli/wire-goldens.json';

const request = () => validateLaunchRequest(structuredClone(golden.validLaunchRequests[0].value));
const bridgeKey = '__CC_DESK_DOCUMENT__';
function bridge(invoke: (...args: any[]) => Promise<unknown>) {
  Object.defineProperty(window, bridgeKey, { configurable: true, value: { invoke, instanceId: 'backend-api' } });
}
beforeEach(() => mockIPC(() => { throw new Error("unguarded invoke"); }));
afterEach(() => { clearMocks(); delete (window as any)[bridgeKey]; });

// 正式 API 必须保留预建的 Channel 身份、原始请求与真实命令名称。
it('D11_Api_StartPreservesPrebuiltChannel_001', async () => {
  const r = request();
  const channel = new Channel();
  const invoke = vi.fn(async () => 'receipt');
  bridge(invoke);
  expect(await api.cliStart(r, channel)).toBe('receipt');
  expect(invoke.mock.calls).toEqual([['cli_start', r, channel]]);
});

// 获取丢失回执只查询原 requestId，不隐式发送启动请求。
it('D11_Api_StatusUsesOriginalRequestId_002', async () => {
  const invoke = vi.fn(async () => 'retained');
  bridge(invoke);
  expect(await api.cliGetLaunchStatus('original')).toBe('retained');
  expect(invoke.mock.calls).toEqual([['cli_get_launch_status', { requestId: 'original' }]]);
});

// 没有可信 bootstrap 时失败，不转入不带文档证明的通用 invoke。
it('D11_Api_MissingDocumentDoesNotFallback_003', async () => {
  await expect(api.cliGetLaunchStatus('original')).rejects.toMatchObject({ code: 'DOCUMENT_BRIDGE_UNAVAILABLE' });
});

// 传输异常只有一次投递，原异常不会被另一次启动遮盖。
it('D11_Api_TransportRejectionNeverRetries_004', async () => {
  const failure = { code: 'transport-test' };
  const invoke = vi.fn(async () => { throw failure; });
  bridge(invoke);
  await expect(api.cliStart(request(), new Channel())).rejects.toBe(failure);
  expect(invoke).toHaveBeenCalledTimes(1);
});

// 本次文档、实例和预建 Channel 被同一个幂等 attempt 固定，丢包后只查原请求。
it('D11_Api_AttemptRecoversWithoutRespawn_005', async () => {
  const r = request();
  const channel = new Channel();
  const receipt = { instanceId: 'backend-api', requestId: r.requestId,
    run: { runId: r.runId, generation: r.generation }, revision: '2', phase: 'running', failure: null };
  const invoke = vi.fn(async (command: string) => {
    if (command === 'cli_start') throw new Error('lost');
    return receipt;
  });
  bridge(invoke);
  const attempt = api.createCliLaunchAttempt(r, channel);
  const first = attempt.start();
  expect(attempt.start()).toBe(first);
  await expect(first).rejects.toThrow('LAUNCH_STATE_UNKNOWN');
  expect(await attempt.recover()).toEqual(receipt);
  expect(invoke.mock.calls).toEqual([
    ['cli_start', r, channel], ['cli_get_launch_status', { requestId: r.requestId }],
  ]);
});

// 旧 attempt 不从替换后的 window 全局对象获取新 bridge，也不接受另一实例的回执。
it('D11_Api_AttemptPinsDocumentAndRejectsRestart_006', async () => {
  const r = request();
  const invoke = vi.fn(async () => ({ instanceId: 'restarted', requestId: r.requestId,
    run: { runId: r.runId, generation: r.generation }, revision: '2', phase: 'running', failure: null }));
  bridge(invoke);
  const attempt = api.createCliLaunchAttempt(r, new Channel());
  const replacement = vi.fn(async () => 'not-called');
  bridge(replacement);
  await expect(attempt.start()).rejects.toThrow('BACKEND_INSTANCE_CHANGED');
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(replacement).not.toHaveBeenCalled();
});
