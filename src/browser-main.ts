import {createApp,defineComponent,h} from 'vue'
import RemoteTerminalPanel from './components/settings/RemoteTerminalPanel.vue'
import i18n from './i18n'
import {browserConnectionSource} from '../remote/ui-terminal/src/browserSource'
import type {ApprovedBrowserInputs} from '../remote/ui-terminal/src/browserEntry'
import {controlledBrowserProfileSource,controlledBrowserReconnectSession} from '../remote/ui-terminal/src/browserProfileSource'
import './styles/global.css'
import './styles/browser-remote.css'
/** An approved same-origin bootstrap may pass existing handles; no enrollment UI. */
export function mountBrowserRemote(element:HTMLElement,inputs?:ApprovedBrowserInputs){
 const source=inputs?browserConnectionSource(inputs):controlledBrowserProfileSource()
 return createApp(defineComponent({setup:()=>()=>h(RemoteTerminalPanel,{source,reconnectSession:inputs?undefined:(signal:AbortSignal)=>controlledBrowserReconnectSession({mode:'safe-idle-retry-v1',budgetMs:60000},signal)})})).use(i18n).mount(element)
}
const element=document.getElementById('browser-remote')
if(element)mountBrowserRemote(element)
