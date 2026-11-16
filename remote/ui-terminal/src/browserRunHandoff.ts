import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import {createStoredBrowserProfileReader,type createBrowserEnrollmentStore} from './browserEnrollmentStore.ts';
import type {BrowserProfileHandles} from './browserPairingProfile.ts';
import type {BrowserResumeGrantConfiguration} from './browserGrant.ts';

/** 显式受控恢复入口：完整匹配原配置后，仅让原认证 TLS 的新 grant 提案提供 scope。
 * 公开候选、存储快照和此策略标记都不批准新 run；原呈现、WebAuthn 与 Host Yes 仍必需。 */
export function createStoredBrowserRunProfileReader(candidateBytes:Uint8Array,store:ReturnType<typeof createBrowserEnrollmentStore>,present:BrowserProfileHandles['present']){
 const read=createStoredBrowserProfileReader(candidateBytes,store,present);
 let resumed:ApprovedBrowserInputs<BrowserResumeGrantConfiguration>|undefined;
 return async(signal:AbortSignal):Promise<ApprovedBrowserInputs<BrowserResumeGrantConfiguration>|undefined>=>{
  // 每次都经过原 reader 的 signal/lease 检查，缓存不能绕过 Forget 或原拥有者退出。
  const original=await read(signal);if(!original)return undefined;
  if(!resumed){const {scope:_scope,...identity}=original.grant;resumed=approvedBrowserInputs({...original,grant:{...identity,scopePolicy:'authenticated-host-run'}});}
  return resumed;
 };
}
