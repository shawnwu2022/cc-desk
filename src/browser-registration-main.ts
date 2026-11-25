import {createApp,defineComponent,h,ref,onBeforeUnmount} from 'vue'
import {controlledBrowserRegistrationSource} from '../remote/ui-terminal/src/browserRegistrationSource'
import './styles/global.css'
import './styles/browser-remote.css'

const element=document.getElementById('browser-registration')
if(element)createApp(defineComponent({setup(){
 const status=ref('Locked'),started=ref(false),busy=ref(false),prepared=ref(false),preparing=ref(false),controller=new AbortController(),source=controlledBrowserRegistrationSource()
 let owner:Awaited<ReturnType<typeof source.register>>
 source.signal.addEventListener('abort',()=>{prepared.value=false;status.value='Locked: preparation cancelled or expired. Reload to prepare a new identity.'})
 const close=()=>{prepared.value=false;status.value='Locked: cancelled. Reload to prepare a new identity.';controller.abort();source.close();owner?.close()}
 onBeforeUnmount(close)
 async function prepare(event:Event){
  if(!event.isTrusted||preparing.value)return
  preparing.value=true;busy.value=true;status.value='Preparing identity'
  try{await source.prepare(controller.signal);prepared.value=true;status.value='Approved for 60 seconds while visible. Start registration.'}catch{status.value='Locked'}finally{busy.value=false}
 }
 async function register(event:Event){
  if(!event.isTrusted||started.value||!prepared.value)return
  started.value=true;busy.value=true;status.value='Registration pending'
  const now=performance.timeOrigin+performance.now()
  try{
   owner=await source.register({signal:controller.signal,setupDeadline:now+10000,registrationDeadline:now+120000})
   if(!owner){status.value='Locked';return}
   const result=await owner.install();await owner.waitClosed()
   status.value=`Browser profile stored (revision ${result.revision}). Open the terminal page to request a connection.`
  }catch{status.value='Locked'}finally{busy.value=false}
 }
 return()=>h('main',{'data-browser-registration':'',class:'browser-registration-shell'},[
  h('section',{class:'browser-registration-card','aria-labelledby':'browser-registration-title'},[
   h('header',[
    h('p',{class:'browser-mobile-kicker'},'CC Desk · Remote'),
    h('h1',{id:'browser-registration-title'},'Pair this device'),
   ]),
   h('ol',[
    h('li','Prepare a device identity and review the connection details.'),
    h('li','After approval, start registration while keeping this page visible.'),
   ]),
   h('p',{class:'browser-registration-status',role:'status'},status.value),
   h('div',{class:'browser-registration-actions'},[
    h('button',{type:'button',class:!prepared.value?'browser-primary':undefined,disabled:preparing.value,onClick:prepare},'Prepare registration'),
    h('button',{type:'button',class:prepared.value?'browser-primary':undefined,disabled:started.value||!prepared.value,onClick:register},'Start registration'),
    h('button',{type:'button',disabled:!busy.value&&!prepared.value,onClick:close},'Cancel'),
   ]),
   h('a',{href:'./browser-remote.html',class:'browser-terminal-link'},'Open terminal'),
   h('p','Registration and browser storage require separate approval. This page does not connect a terminal.'),
  ]),
 ])
}})).mount(element)
