<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref, shallowRef } from 'vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { nativeObserverSetup } from '@/api/tauri'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import { DESKTOP_PAIRING_PORT, type ApprovedDevice, type DesktopPairingPort, type PairingSnapshot, type PendingPairing, type SetupResult, type VerifiedPairing } from './desktopPairingPort'

const { t } = useI18n()
const tabs = useNativeTabsStore()
const selectedShares = ref<string[]>([])
const enableControl = ref(false)
const shareChoices = computed(() => [...tabs.tabs.values()]
  .filter(tab => tab.status === 'running' && tab.runId && tab.generation > 0)
  .map((tab, index) => ({ id: tab.tabId + ':' + tab.generation, title: tab.title, runId: tab.runId, generation: tab.generation,
    label: ({ codex: 'Codex', claude: 'Claude', opencode: 'OpenCode' } as Record<string, string>)[tab.cli] + ' ' + (index + 1) })))
function shareWith(device: ApprovedDevice) {
  const selected = selectedShares.value.map(id => shareChoices.value.find(choice => choice.id === id))
  if (selected.length === 0 || selected.length > 4 || selected.some(choice => !choice)) return
  const role = enableControl.value ? 'Controller' as const : undefined
  enableControl.value = false
  void perform(handle => handle.share!(device, selected.map(choice => ({ runId: choice!.runId, generation: choice!.generation, label: choice!.label, ...(role ? { role } : {}) }))), true)
}
const injectedPort = inject(DESKTOP_PAIRING_PORT, null)
const port = shallowRef<DesktopPairingPort | null>(injectedPort)
async function initializeNativeSetup() {
  if (port.value) return
  try {
    const result = await nativeObserverSetup({ action: 'available' }) as { kind?: unknown }
    if (!disposed && result.kind === 'available') port.value = {
      async provision(endpoint) {
        const result=await nativeObserverSetup({action:'provision',endpoint}) as {kind?:unknown;metadata?:unknown}
        if(result.kind==='declined')return null
        if(result.kind!=='publicRegistration')throw new Error('NATIVE_SETUP_UNCONFIRMED')
        return JSON.stringify(result.metadata,null,2)
      },
      async verify(endpoint) {
        const reply = await nativeObserverSetup({ action: 'verify', endpoint }) as { kind?: unknown; ticket?: unknown; endpoint?: unknown }
        if (reply.kind === 'profileRequired' || reply.kind === 'declined') return { kind: reply.kind }
        if (reply.kind === 'verified' && typeof reply.ticket === 'string' && typeof reply.endpoint === 'string') {
          return { kind: 'verified', handle: boundNativePairing(reply.ticket, reply.endpoint) }
        }
        if (reply.kind !== 'relayAuthorizationRequired') throw new Error('NATIVE_OBSERVER_SETUP_UNAVAILABLE')
        return { kind: 'relayAuthorizationRequired', async approvePublicRegistration() {
          const value = await nativeObserverSetup({ action: 'publicRegistration', endpoint }) as { kind?: unknown; metadata?: unknown }
          return value.kind === 'publicRegistration' ? JSON.stringify(value.metadata, null, 2) : null
        } }
      },
    }
  } catch { /* Unsupported builds have no native capability. */ }
}
function boundNativePairing(ticket: string, origin: string): VerifiedPairing {
  async function request(action: string, args: Record<string, unknown> = {}) {
    const reply = await nativeObserverSetup({ action, ticket, ...args }) as { kind?: unknown; snapshot?: PairingSnapshot }
    if (reply.kind !== 'snapshot' || !reply.snapshot) throw new Error('NATIVE_OBSERVER_STATE_UNCONFIRMED')
    return reply.snapshot
  }
  const target = ({ tunnel, phoneId, phoneRelayPublicKeySec1, phoneEndpointSpki, endpointBinding }: ApprovedDevice) => ({ tunnel, phoneId, phoneRelayPublicKeySec1, phoneEndpointSpki, endpointBinding })
  return {
    endpoint: origin,
    current: () => request('current'),
    create: minutes => request('create', { minutes }),
    cancel: inviteId => request('cancel', { inviteId }),
    approve: (target, scopeChoiceId) => request('approve', { target, scopeChoiceId }),
    reject: target => request('reject', { target }),
    revoke: device => request('revoke', { target: target(device) }),
    share: (device, runs) => request('startSharing', { target: target(device), runs }),
    stopSharing: () => request('stopSharing'),
  }
}
const endpoint = ref('')
const minutes = ref('5')
const selectedScope = ref('')
const busy = ref(false)
const failure = ref<'verify' | 'operation' | null>(null)
const verified = shallowRef<VerifiedPairing | null>(null)
const setup = shallowRef<Exclude<SetupResult, { kind: 'verified' }> | null>(null)
const publicRegistration = ref('')
const setupCopied = ref(false)
const snapshot = shallowRef<PairingSnapshot | null>(null)
const now = ref(Date.now())
let disposed = false
let ticker: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  ticker = setInterval(() => { if (invite.value) now.value = Date.now() }, 1000)
  void initializeNativeSetup()
})
onUnmounted(() => { disposed = true; if (ticker !== undefined) clearInterval(ticker) })

const ttlOptions = computed(() => [1, 5, 10].map(value => ({ value: String(value), label: t('remotePairingMinutes', { value }) })))
const scopeOptions = computed(() => [
  { value: '', label: t('remotePairingScopePlaceholder') },
  ...(snapshot.value?.scopeChoices ?? []).map(scope => ({ value: scope.id, label: scope.label })),
])
const scopeValid = computed(() => !selectedScope.value || !!snapshot.value?.scopeChoices.some(scope => scope.id === selectedScope.value))
const invite = computed(() => snapshot.value?.invite ?? null)
const inviteLive = computed(() => !!invite.value && invite.value.expiresUnixMs > now.value)
const timeLeft = computed(() => {
  const seconds = Math.max(0, Math.ceil(((invite.value?.expiresUnixMs ?? 0) - now.value) / 1000))
  return Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0')
})
const code = computed(() => {
  const value = invite.value?.manualCode
  return value && /^CCDESK1-[0-9A-HJKMNP-TV-Z]{16}-[0-9A-HJKMNP-TV-Z]{20}$/.test(value) ? value : ''
})
const qr = computed(() => {
  const rows = invite.value?.qrRows
  if (!rows || rows.length < 21 || rows.length > 177 || (rows.length - 21) % 4 !== 0
    || rows.some(row => row.length !== rows.length || !/^[01]+$/.test(row))) return null
  const path = rows.flatMap((row, y) => Array.from(row, (bit, x) => bit === '1' ? ('M' + (x + 4) + ' ' + (y + 4) + 'h1v1h-1z') : '')).join('')
  return { viewBox: '0 0 ' + (rows.length + 8) + ' ' + (rows.length + 8), path }
})

function setEndpoint(value: string) {
  if (busy.value) return
  endpoint.value = value
  selectedScope.value = ''
  setup.value = null
  publicRegistration.value = ''
  verified.value = null
  snapshot.value = null
  failure.value = null
}
async function verify() {
  const input = endpoint.value.trim()
  const bridge = port.value
  if (!bridge || !input.startsWith('wss://') || busy.value) return
  verified.value = null
  snapshot.value = null
  selectedScope.value = ''
  failure.value = null
  setup.value = null
  publicRegistration.value = ''
  busy.value = true
  try {
    const result = await bridge.verify(input)
    if (disposed) return
    if (result.kind !== 'verified') { setup.value = result; return }
    const handle = result.handle
    const state = await handle.current()
    if (!disposed) { verified.value = handle; snapshot.value = state; now.value = Date.now() }
  } catch {
    if (!disposed) failure.value = 'verify'
  } finally {
    if (!disposed) busy.value = false
  }
}
async function provisionProfile() {
  const bridge=port.value
  if(!bridge?.provision || busy.value)return
  busy.value=true
  failure.value=null
  try{
    const value=await bridge.provision(endpoint.value.trim())
    if(!disposed && value)publicRegistration.value=value
  }catch{if(!disposed)failure.value='verify'}
  finally{if(!disposed)busy.value=false}
}
async function copyRegistration(){
  if(!publicRegistration.value)return
  try {await navigator.clipboard.writeText(publicRegistration.value); setupCopied.value=true}
  catch { setupCopied.value=false }
}
async function reopenRegistration(){
  if(busy.value || !endpoint.value.trim().startsWith('wss://'))return
  busy.value=true; failure.value=null; setupCopied.value=false
  try {
    const value=await nativeObserverSetup({action:'publicRegistration',endpoint:endpoint.value.trim()}) as {kind?:unknown;metadata?:unknown}
    if(disposed)return
    if(value.kind==='publicRegistration')publicRegistration.value=JSON.stringify(value.metadata,null,2)
    else if(value.kind==='profileRequired')setup.value={kind:'profileRequired'}
    else if(value.kind!=='declined')failure.value='verify'
  }catch{if(!disposed)failure.value='verify'}
  finally{if(!disposed)busy.value=false}
}
async function showPublicRegistration() {
  const task = setup.value
  if (!task || task.kind !== 'relayAuthorizationRequired' || busy.value || disposed) return
  busy.value = true
  publicRegistration.value = ''
  failure.value = null
  try {
    const value = await task.approvePublicRegistration()
    if (!disposed && setup.value === task && value) publicRegistration.value = value
  } catch {
    if (!disposed) failure.value = 'operation'
  } finally {
    if (!disposed) busy.value = false
  }
}
async function perform(operation: (handle: VerifiedPairing) => Promise<PairingSnapshot>, readAfterShareFailure = false) {
  const handle = verified.value
  if (!handle || busy.value || disposed) return
  failure.value = null
  busy.value = true
  try {
    const state = await operation(handle)
    if (!disposed && verified.value === handle) {
      if (!state.scopeChoices.some(scope => scope.id === selectedScope.value)) selectedScope.value = ''
      snapshot.value = state
      now.value = Date.now()
    }
  } catch {
    if (!disposed && verified.value === handle) {
      // Unknown operation outcome: hide old codes and decisions until a fresh read.
      snapshot.value = null
      failure.value = 'operation'
      if (readAfterShareFailure) {
        // 只核对实际状态，不重发可能已生效的共享请求。
        try {
          const state = await handle.current()
          if (!disposed && verified.value === handle) {
            if (!state.scopeChoices.some(scope => scope.id === selectedScope.value)) selectedScope.value = ''
            snapshot.value = state
            now.value = Date.now()
          }
        } catch { /* 保持过期操作入口隐藏，等待用户主动刷新。 */ }
      }
    }
  } finally {
    if (!disposed) busy.value = false
  }
}
function create() {
  const value = Number(minutes.value)
  if (snapshot.value && !inviteLive.value && (value === 1 || value === 5 || value === 10)) void perform(handle => handle.create(value))
}
function decide(request: PendingPairing, approve: boolean) {
  const active = invite.value
  if (active && active.inviteId === request.inviteId && active.expiresUnixMs > Date.now()) {
    if (!approve) { void perform(handle => handle.reject(request)); return }
    const scope = selectedScope.value
    if (scopeValid.value) void perform(handle => handle.approve(request, scope || null))
  }
}
function cancel() {
  const active = invite.value
  if (active) void perform(handle => handle.cancel(active.inviteId))
}
</script>
<template>
  <section class="remote-connections" data-remote-connections :aria-label="t('remoteConnections')">
    <h1>{{ t('remoteConnections') }}</h1>
    <p v-if="!port" role="status">{{ t('remotePairingUnavailable') }}</p>
    <section v-else class="pairing" :aria-label="t('remotePairingTitle')">
      <h2>{{ t('remotePairingTitle') }}</h2>
      <p>{{ t('remotePairingObserverHint') }}</p>
      <AppInput :model-value="endpoint" :label="t('remotePairingEndpoint')" :disabled="busy"
        autocomplete="off" spellcheck="false" maxlength="2048" @update:model-value="setEndpoint" />
      <div class="actions">
        <AppButton v-if="!verified" variant="primary" :disabled="busy || !endpoint.trim().startsWith('wss://')" @click="verify">{{ t('remotePairingVerify') }}</AppButton>
        <template v-else>
          <AppButton variant="ghost" :disabled="busy" @click="perform(handle => handle.current())">{{ t('remotePairingRefresh') }}</AppButton>
          <AppButton v-if="failure" variant="ghost" :disabled="busy" @click="verify">{{ t('remotePairingVerify') }}</AppButton>
        </template>
      </div>
      <p v-if="verified" role="status">{{ t('remotePairingVerified', { endpoint: verified.endpoint }) }}</p>
      <p v-if="failure" role="alert">{{ t(failure === 'verify' ? 'remotePairingVerifyFailed' : 'remotePairingFailed') }}</p>
      <p v-if="setup?.kind === 'declined'" role="status">{{ t('remotePairingSetupDeclined') }}</p>
      <div v-if="!verified && setup?.kind !== 'relayAuthorizationRequired'" class="actions">
        <AppButton variant="ghost" :disabled="busy || !endpoint.trim().startsWith('wss://')" @click="reopenRegistration">{{ t('remotePairingPublicRegistration') }}</AppButton>
      </div>
      <div v-if="setup?.kind === 'profileRequired'" class="pairing">
        <p role="status">{{ t(publicRegistration ? 'remotePairingAuthRequired' : 'remotePairingProfileRequired') }}</p>
        <AppButton v-if="port?.provision && !publicRegistration" :disabled="busy" @click="provisionProfile">{{ t('remotePairingProvision') }}</AppButton>
      </div>
      <div v-if="setup?.kind === 'relayAuthorizationRequired'" class="pairing" role="status">
        <p>{{ t('remotePairingAuthRequired') }}</p>
        <div class="actions"><AppButton :disabled="busy" @click="showPublicRegistration">{{ t('remotePairingPublicRegistration') }}</AppButton></div>

      </div>
      <div v-if="publicRegistration && !verified" class="pairing">
        <p>{{ t('remotePairingSetupHint') }}</p>
        <label>{{ t('remotePairingCopyPublic') }}<textarea :value="publicRegistration" readonly spellcheck="false" rows="4" /></label>
        <div class="actions"><AppButton @click="copyRegistration">{{ t(setupCopied ? 'remotePairingCopied' : 'remotePairingCopy') }}</AppButton></div>
      </div>
      <template v-if="snapshot">
        <template v-if="invite">
          <h3>{{ t('remotePairingInvite') }}</h3>
          <p>{{ inviteLive ? t('remotePairingExpires', { time: timeLeft }) : t('remotePairingExpired') }}</p>
          <template v-if="inviteLive && !busy">
            <p v-if="qr">{{ t('remotePairingQr') }}</p>
            <svg v-if="qr" class="pairing-qr" :viewBox="qr.viewBox" role="img"
              :aria-label="t('remotePairingQr')" shape-rendering="crispEdges">
              <rect width="100%" height="100%" fill="#fff" />
              <path :d="qr.path" fill="#000" />
            </svg>
            <details v-if="code"><summary>{{ t('remotePairingCode') }}</summary><code>{{ code }}</code></details>
          </template>
          <AppButton :disabled="busy" @click="cancel">{{ t('remotePairingCancel') }}</AppButton>
        </template>
        <template v-if="!inviteLive">
          <div class="actions"><AppButton variant="primary" :disabled="busy" @click="create">{{ t(invite ? 'remotePairingRegenerate' : 'remotePairingCreate') }}</AppButton></div>
          <details><summary>{{ t('remotePairingAdvanced') }}</summary>
            <AppSelect v-model="minutes" :label="t('remotePairingTtl')" :options="ttlOptions" :disabled="busy" />
          </details>
        </template>
        <h3>{{ t('remotePairingPending') }}</h3>
        <p v-if="snapshot.pending.length === 0">{{ t('remotePairingEmpty') }}</p>
        <template v-else>
          <AppSelect v-if="snapshot.scopeChoices.length > 0" v-model="selectedScope" :label="t('remotePairingScope')" :options="scopeOptions" :disabled="busy" />
          <ul class="devices">
          <li v-for="request in snapshot.pending" :key="request.requestId">
            <p>{{ t('remotePairingPhone') }}</p>
            <details><summary>{{ t('remotePairingDetails') }}</summary><dl>
              <dt>{{ t('remotePairingPhoneId') }}</dt><dd>{{ request.phoneId }}</dd>
              <dt>{{ t('remotePairingRelayKey') }}</dt><dd><code>{{ request.phoneRelayPublicKeySec1 }}</code></dd>
              <dt>{{ t('remotePairingEndpointKey') }}</dt><dd><code>{{ request.phoneEndpointSpki }}</code></dd>
              <dt>{{ t('remotePairingBinding') }}</dt><dd><code>{{ request.endpointBinding }}</code></dd>
            </dl></details>
            <div class="actions">
              <AppButton variant="primary" :disabled="busy || !inviteLive || !scopeValid || request.inviteId !== invite?.inviteId"
                @click="decide(request, true)">{{ t('remotePairingApprove') }}</AppButton>
              <AppButton :disabled="busy || !inviteLive || request.inviteId !== invite?.inviteId"
                @click="decide(request, false)">{{ t('remotePairingReject') }}</AppButton>
            </div>
          </li>
        </ul>
        </template>
        <h3>{{ t('remotePairingDevices') }}</h3>
        <fieldset v-if="verified?.share && snapshot.devices.some(device => !device.revocationPending)">
          <legend>{{ t('remotePairingShareChoose') }}</legend>
          <p v-if="shareChoices.length === 0">{{ t('remotePairingShareNoRuns') }}</p>
          <label v-if="selectedShares.length" class="share-choice"><input v-model="enableControl" type="checkbox" :disabled="busy" /> {{ t('remotePairingAllowControl') }}</label>
          <label v-for="choice in shareChoices" :key="choice.id" class="share-choice">
            <input v-model="selectedShares" type="checkbox" :value="choice.id" :disabled="busy || (!selectedShares.includes(choice.id) && selectedShares.length >= 4)" />
            {{ choice.title }} ({{ choice.label }})
          </label>
        </fieldset>
        <p v-if="snapshot.devices.length === 0">{{ t('remotePairingEmpty') }}</p>
        <ul v-else class="devices">
          <li v-for="device in snapshot.devices" :key="device.tunnel">
            <p>{{ t('remotePairingPhone') }} {{ device.phoneId.slice(-6) }}</p>
            <p v-if="device.revocationPending" role="status">{{ t('remotePairingRevokePending') }}</p>
            <p v-else>{{ t(device.sharing === 'controlling' ? 'remotePairingControlActive' : device.sharing === 'active' ? 'remotePairingShareActive' : device.sharing === 'offered' ? 'remotePairingShareOffered' : device.sharing === 'unknown' ? 'remotePairingShareUnknown' : 'remotePairingNoScope') }}</p>
            <details><summary>{{ t('remotePairingDetails') }}</summary><dl>
              <dt>{{ t('remotePairingPhoneId') }}</dt><dd>{{ device.phoneId }}</dd>
              <dt>{{ t('remotePairingRelayKey') }}</dt><dd><code>{{ device.phoneRelayPublicKeySec1 }}</code></dd>
              <dt>{{ t('remotePairingEndpointKey') }}</dt><dd><code>{{ device.phoneEndpointSpki }}</code></dd>
              <dt>{{ t('remotePairingBinding') }}</dt><dd><code>{{ device.endpointBinding }}</code></dd>
            </dl></details>
            <div class="actions">
              <AppButton v-if="verified?.share && device.sharing === 'none' && !device.revocationPending" :disabled="busy || selectedShares.length === 0" @click="shareWith(device)">{{ t('remotePairingShareStart') }}</AppButton>
              <AppButton v-if="verified?.stopSharing && device.sharing !== 'none'" :disabled="busy" @click="perform(handle => handle.stopSharing!())">{{ t('remotePairingShareStop') }}</AppButton>
              <AppButton variant="danger" :disabled="busy" @click="perform(handle => handle.revoke(device))">{{ t('remotePairingRevoke') }}</AppButton>
            </div>
          </li>
        </ul>
      </template>
    </section>
  </section>
</template>
<style scoped>
.remote-connections { display: grid; align-content: start; gap: 16px; min-width: 0; overflow: auto; padding: 24px; }
h1, h2, h3, p, dl, dd { margin: 0; }
h1 { font-size: 20px; color: var(--text-primary); }
p, dl { overflow-wrap: anywhere; color: var(--text-secondary); }
.pairing { display: flex; flex-direction: column; gap: 12px; width: min(100%, 720px); }
.share-choice { display: block; margin: 4px 0; }
.actions { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; }
.pairing-qr { width: min(100%, 240px); aspect-ratio: 1; }
.devices { display: grid; gap: 12px; margin: 0; padding: 0; list-style: none; }
.devices li { display: grid; gap: 8px; padding: 12px; border: 1px solid var(--border-color); }
dd { margin-bottom: 4px; }
textarea { display: block; width: 100%; box-sizing: border-box; color: var(--text-primary); background: var(--bg-secondary); border: 1px solid var(--border-color); }
</style>
