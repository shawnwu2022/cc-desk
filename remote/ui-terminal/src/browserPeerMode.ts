export type BrowserPeerInstall=Readonly<{budgetMs:number;controlLease?:'manual-v1'|'live-v1';snapshotPull?:'snapshot-pull-v1'}>;
// 仅选择认证协议模式，不授予权限；未填写时保持旧模式。
export function validBrowserPeerInstall(value:unknown,role:unknown):value is BrowserPeerInstall{
 if(!value||typeof value!=='object')return false;
 const v=value as Record<string,unknown>,keys=Object.keys(v);
 const lease=Object.prototype.hasOwnProperty.call(v,'controlLease'),pull=Object.prototype.hasOwnProperty.call(v,'snapshotPull');
 if(keys.length!==1+Number(lease)+Number(pull)||!keys.every(k=>k==='budgetMs'||k==='controlLease'||k==='snapshotPull')||
  !Object.prototype.hasOwnProperty.call(v,'budgetMs')||!Number.isInteger(v.budgetMs)||Number(v.budgetMs)<1||Number(v.budgetMs)>10000||
  ('controlLease'in v&&!lease)||('snapshotPull'in v&&!pull))return false;
 if(lease&&((v.controlLease!=='manual-v1'&&v.controlLease!=='live-v1')||role!=='Controller'))return false;
 return !pull||(v.snapshotPull==='snapshot-pull-v1'&&lease&&v.controlLease==='live-v1'&&role==='Controller');
}