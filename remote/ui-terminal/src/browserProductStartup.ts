import {approvedBrowserInputs,type ApprovedBrowserInputs} from './browserEntry.ts';
import {createBrowserEnrollmentStore} from './browserEnrollmentStore.ts';
import {createBrowserGrantPresenter} from './browserGrantPresenter.ts';
import {createInstalledBrowserEnrollmentProfileReader,type BrowserProfileHandles} from './browserPairingProfile.ts';
import type {BrowserResumeGrantConfiguration} from './browserGrant.ts';

// 独立批准计划须显式选择此产品库；不能改写已经批准的安装目标。
export const BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME='CC_DESK_BROWSER_ENROLLMENT_V1';
type Store=ReturnType<typeof createBrowserEnrollmentStore>;
type Session=NonNullable<Awaited<ReturnType<Store['openExisting']>>>;
type Inputs=ApprovedBrowserInputs<BrowserResumeGrantConfiguration>;
const unavailable=()=>Error('browser product profile unavailable');

/** 原安装记录是唯一批准来源；一次打开、一次读取，不使用候选、URL 或回执选择权限。 */
export function createInstalledBrowserRunProfileReader(store:Store,present:BrowserProfileHandles['present']){
 let original:Promise<Inputs|undefined>|undefined,firstSignal:AbortSignal|undefined,session:Session|undefined;
 return async(signal:AbortSignal):Promise<Inputs|undefined>=>{
  if(!original){
   firstSignal=signal;
   original=Promise.resolve().then(async()=>{
    if(!(signal instanceof AbortSignal)||signal.aborted)throw unavailable();
    session=await store.openExisting(present,signal);if(!session)return undefined;
    const approved=await createInstalledBrowserEnrollmentProfileReader(session.provider)(signal);
    if(!approved)throw unavailable();
    const {scope:_scope,...identity}=approved.grant;
    return approvedBrowserInputs({...approved,grant:{...identity,scopePolicy:'authenticated-host-run'}});
   });
   void original.catch(()=>{});
  }
  try{
   const value=await original;
   // 缓存不能绕过本次取消、原请求取消或原租约撤销。
   if(!(signal instanceof AbortSignal)||signal.aborted||firstSignal!.aborted||session?.provider.enrollment.signal.aborted)throw unavailable();
   return value;
  }catch{
   // 失败加入原 session 的真实退出；不能用超时或另建 owner 宣称退休完成。
   if(session){session.close();await session.waitClosed();}
   throw unavailable();
  }
 };
}

/** 构造及模块导入无浏览器副作用；仅显式 Connect 的读取创建 store/presenter。 */
export function createProductBrowserProfileReader(){
 let original:Promise<ReturnType<typeof createInstalledBrowserRunProfileReader>>|undefined;
 return async(signal:AbortSignal):Promise<Inputs|undefined>=>{
  if(!original){
   original=Promise.resolve().then(()=>{
    if(!(signal instanceof AbortSignal)||signal.aborted)throw unavailable();
    const origin=location.origin;
    return createInstalledBrowserRunProfileReader(createBrowserEnrollmentStore({origin,databaseName:BROWSER_PRODUCT_ENROLLMENT_DATABASE_NAME}),createBrowserGrantPresenter(origin));
   });
   void original.catch(()=>{});
  }
  const read=await original;
  return read(signal);
 };
}
