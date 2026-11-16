export type BrowserPeerInstall=Readonly<{budgetMs:number;controlLease?:'manual-v1'}>;
// 这里只选择认证协议模式，不产生控制权限，也不从配对状态推断开启。
export function validBrowserPeerInstall(value:unknown,role:unknown):value is BrowserPeerInstall{
 if(!value||typeof value!=='object')return false;
 const v=value as Record<string,unknown>,keys=Object.keys(v),manual=Object.prototype.hasOwnProperty.call(v,'controlLease');
 return keys.length===(manual?2:1)&&keys.every(k=>k==='budgetMs'||(manual&&k==='controlLease'))&&Object.prototype.hasOwnProperty.call(v,'budgetMs')&&Number.isInteger(v.budgetMs)&&Number(v.budgetMs)>=1&&Number(v.budgetMs)<=10000&&(!('controlLease'in v)||manual)&&(manual?v.controlLease==='manual-v1'&&role==='Controller':true);
}
