<script lang="ts">
// 同一文档的所有面板共享递增序号；关开/卸载不能重用旧取消序号。
type ForgetIdentity = { readonly requestId: string; readonly requestSequence: string }
let forgetSequence = 0n
function nextForgetIdentity(): ForgetIdentity {
  if (forgetSequence >= 18446744073709551615n) throw new Error('FORGET_SEQUENCE_EXHAUSTED')
  const requestId = crypto.randomUUID()
  return { requestId, requestSequence: (++forgetSequence).toString() }
}
</script>
<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef, watch } from 'vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { parseJson } from '../../../remote/ui-terminal/src/wire'
import BrowserPairingCandidateEditor from './BrowserPairingCandidateEditor.vue'
import ShortRemoteSessionPanel from './ShortRemoteSessionPanel.vue'
import { captureNativeAttempt, matchesNativeAttempt, useNativeTabsStore, type NativeAttemptIdentity } from '@/stores/nativeTabs'
import { remoteHostConfigure, remoteHostCurrent, remoteHostRequest, remoteHostStart, remoteHostCancel, remoteHostDiscardCandidate, remoteHostPairingScope } from '@/api/tauri'
import { useNotificationsStore } from '@/stores/notifications'
import { remotePairingList, remotePairingSelect, remotePairingForget, remotePairingForgetCancel, type RemotePairingList, type RemotePairingInstallation, remoteRegistrationRequest, remoteRegistrationCurrent, remoteRegistrationCancel } from '@/api/tauri'
const notifications = useNotificationsStore()
function cleanupFailed() { notifications.pushToast({ kind: 'warning', messageKey: 'remoteHostCleanupUnconfirmed' }) }
const props = defineProps<{ tabId: string }>()
const tabs = useNativeTabsStore()
const open = ref(false), candidate = ref(''), busy = ref(false), configured = ref(false)
const state = ref('Locked'), message = ref('')
const pairingScope = ref('')
const registrationJson = ref(''), registrationState = ref('Locked')
const pendingForget = shallowRef<ForgetIdentity | null>(null)
const installationLabel = (value: RemotePairingInstallation) => value === 'confirmed' ? '历史安装已确认' : value === 'pending' ? '安装完成尚未确认' : '安装尚未验证'
const remember = ref(false), devices = ref<RemotePairingList | null>(null)
let registrationRequested = false
let epoch = 0, captured: NativeAttemptIdentity | null = null
let receipt: string | null = null, requested = false
const available = computed(() => { const t = tabs.tab(props.tabId); return t?.status === 'running' && t.generation > 0 })
const shortRun = computed(() => { const t=tabs.tab(props.tabId); return available.value && t?.runId ? {runId:t.runId,generation:t.generation} : null })
const statusText = computed(() => ({ Locked: '尚无批准', Pending: '等待本机原生确认', ApprovalRecorded: '已记录批准，尚未启动', Consumed: '单次启动已受理，不代表已连接', Registered: '已登记，不代表已连接', Cancelled: '已取消', Expired: '已到期' }[state.value] ?? '状态未知'))
function stillCurrent(token: number, attempt: NativeAttemptIdentity) { return token === epoch && open.value && matchesNativeAttempt(tabs.tab(props.tabId), attempt) }
function invalidate() {
  const previous = captured, previousReceipt = receipt, wasRequested = requested
  const wasRegistering = registrationRequested, forgetIdentity = pendingForget.value
  pendingForget.value = null
  registrationRequested = false; remember.value = false; devices.value = null; registrationJson.value = ''; registrationState.value = 'Locked'
  epoch++; captured = null; receipt = null; requested = false; candidate.value = ''; pairingScope.value = ''; configured.value = false; busy.value = false; state.value = 'Locked'; message.value = ''; open.value = false
  if (previous) {
    const run = { runId: previous.runId, generation: previous.generation }
    if (forgetIdentity) void remotePairingForgetCancel({ ...run, candidateJson: JSON.stringify(forgetIdentity) }).catch(cleanupFailed)
    if (wasRegistering) void remoteRegistrationCancel(run).catch(cleanupFailed)
    if (wasRequested) void remoteHostCancel(run).catch(cleanupFailed)
    else if (previousReceipt) void remoteHostDiscardCandidate({ ...run, receipt: previousReceipt }).catch(cleanupFailed)
  }
}
// 先拒绝重复键，再只改顶层 remember；保留其他数字的原始词法，不替候选修复格式。
function registrationCandidate() {
  const raw = registrationJson.value.trim()
  const parsed = parseJson(Uint8Array.from(new TextEncoder().encode(raw)), 16384)
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('INVALID_REGISTRATION')
  let candidateJson: string
  if (Object.prototype.hasOwnProperty.call(parsed, 'remember')) {
    if (typeof (parsed as { remember: unknown }).remember !== 'boolean') throw new Error('INVALID_REMEMBER')
    const tokens = [...raw.matchAll(/"(?:\\.|[^"\\])*"|[{}\[\]:,]|[^{}\[\]:,\s]+/g)]
    let depth = 0, valueToken: RegExpMatchArray | undefined
    for (let i = 0; i < tokens.length; i++) {
      const token = tokens[i][0]
      if (token === '{' || token === '[') depth++
      else if (token === '}' || token === ']') depth--
      else if (depth === 1 && token.startsWith('"') && tokens[i + 1]?.[0] === ':' && JSON.parse(token) === 'remember') { valueToken = tokens[i + 2]; break }
    }
    if (!valueToken || valueToken.index === undefined) throw new Error('INVALID_REMEMBER')
    candidateJson = raw.slice(0, valueToken.index) + String(remember.value) + raw.slice(valueToken.index + valueToken[0].length)
  } else candidateJson = raw.slice(0, -1) + (Object.keys(parsed).length ? ',' : '') + '"remember":' + String(remember.value) + '}'
  if (new TextEncoder().encode(candidateJson).length > 16384) throw new Error('INVALID_REGISTRATION_SIZE')
  return candidateJson
}
async function readDevices() {
  const attempt = captured, token = epoch
  if (!attempt || busy.value || !stillCurrent(token, attempt)) return
  busy.value = true; message.value = ''; devices.value = null
  try {
    const result = await remotePairingList({ runId: attempt.runId, generation: attempt.generation })
    if (stillCurrent(token, attempt)) devices.value = result
  } catch { if (stillCurrent(token, attempt)) message.value = '未能读取已记住设备。不会创建或修复存储，也不会自动重试。' }
  finally { if (stillCurrent(token, attempt)) busy.value = false }
}
function cancelForget() {
  const attempt = captured, identity = pendingForget.value
  if (!attempt || !identity) return
  pendingForget.value = null
  message.value = '已请求取消忘记确认，等待原生操作结束。不会自动重试。'
  void remotePairingForgetCancel({ runId: attempt.runId, generation: attempt.generation, candidateJson: JSON.stringify(identity) }).catch(cleanupFailed)
}
async function useDevice(kind: 'select' | 'forget', deviceId: string) {
  const attempt = captured, token = epoch, snapshot = devices.value
  if (!attempt || busy.value || !stillCurrent(token, attempt) || !snapshot?.storeId || !snapshot.revision || !snapshot.devices.some(device => device.deviceId === deviceId)) return
  if (kind === 'select' && (configured.value || registrationRequested)) return
  const run = { runId: attempt.runId, generation: attempt.generation }
  let identity: ForgetIdentity | null = null
  busy.value = true; message.value = ''
  try {
    identity = kind === 'forget' ? nextForgetIdentity() : null
    const request = { ...run, candidateJson: JSON.stringify({ storeId: snapshot.storeId, revision: snapshot.revision, deviceId, ...(identity ?? {}) }) }
    if (kind === 'select') {
      const result = await remotePairingSelect(request)
      if (!stillCurrent(token, attempt)) { await remoteHostDiscardCandidate({ ...run, receipt: result.receipt }); return }
      receipt = result.receipt; configured.value = true; requested = false; state.value = 'Locked'; candidate.value = ''
      message.value = '已选为当前会话的新候选，仍为 Locked；不恢复旧授权，不打开密钥或连接。请另行请求本机批准。'
    } else {
      pendingForget.value = identity
      const result = await remotePairingForget(request)
      if (!stillCurrent(token, attempt)) return
      if (pendingForget.value !== identity) { devices.value = null; message.value = '忘记确认已结束。请刷新已记住设备核对结果，不会自动重试。'; return }
      pendingForget.value = null
      if (result.state === 'Declined') { message.value = '未确认忘记设备。不会自动重试。'; return }
      if (result.state !== 'Forgotten') throw new Error('INVALID_FORGET_RESPONSE')
      devices.value = null
      message.value = '已忘记设备；不终止独立已批准的连接。'
      try {
        const list = await remotePairingList(run)
        if (stillCurrent(token, attempt)) devices.value = list
      } catch { if (stillCurrent(token, attempt)) message.value = '已忘记设备，但列表刷新失败。请手动刷新；不会重放忘记操作。' }
    }
  } catch {
    if (stillCurrent(token, attempt) && pendingForget.value === identity && identity) cancelForget()
    if (stillCurrent(token, attempt)) { devices.value = null; message.value = '设备操作未确认成功。请刷新已记住设备后核对，不会自动重试。' }
    else if (kind === 'select') cleanupFailed()
  } finally { if (stillCurrent(token, attempt)) busy.value = false }
}
async function register(kind: 'request' | 'refresh' | 'cancel') {
  const attempt = captured, token = epoch
  if (!attempt || busy.value || !stillCurrent(token, attempt)) return
  if (kind === 'request' && (!registrationJson.value || new TextEncoder().encode(registrationJson.value).length > 16384)) { message.value = '登记请求必须为不含 credential 的公开 JSON，且不超过16KiB。'; return }
  let candidateJson = ''
  if (kind === 'request') {
    try { candidateJson = registrationCandidate() }
    catch { message.value = '登记请求格式无效，请核对公开JSON、重复字段及16KiB限制。'; return }
  }
  const run = { runId: attempt.runId, generation: attempt.generation }
  busy.value = true; message.value = ''
  try {
    if (kind === 'request') registrationRequested = true
    const result = kind === 'request'
      ? await remoteRegistrationRequest({ ...run, candidateJson })
      : await (kind === 'cancel' ? remoteRegistrationCancel : remoteRegistrationCurrent)(run)
    if (!stillCurrent(token, attempt)) { if (kind === 'request') await remoteRegistrationCancel(run); return }
    registrationState.value = result.state
    if (result.state === 'NativeStoredRegistrationIncomplete') {
      message.value = '已在本机记住公开设备，但浏览器登记未确认完成。列表仍为 Locked；不会自动重试或恢复旧授权。'
    } else if (result.state === 'PendingNativePersistence') {
      message.value = '等待独立本机存储确认；尚未授予 Host 权限。'
    } else if (result.state === 'RetirementUnknown') {
      message.value = '原登记资源的退休结果不确定。已安装记录（如有）仍为锁定；请勿重试或启动。'
      if (result.remembered) message.value += ' 已在本机记住公开设备。'
    } else if (['NativeStoreDeclined', 'NativeStoreFailed', 'NativeStoreUnknown'].includes(result.state)) {
      message.value = result.state === 'NativeStoreDeclined' ? '本机记住确认已拒绝，登记已停止。不会自动重试。' : result.state === 'NativeStoreUnknown' ? '本机存储结果不确定，登记未确认完成。请只读刷新已记住设备核对，不会自动重试。' : '本机记住失败，登记未确认完成。不会自动重试。'
    } else if ('receipt' in result) {
      receipt = result.receipt; configured.value = true; state.value = 'Locked'; registrationJson.value = ''
      message.value = result.state === 'InstalledLocked' ? '登记已完成，本次会话候选仍为锁定。Host控制需要下方另一次本机批准和启动。' : '本机会话候选已安装并保持锁定，但浏览器交付未确认。不要重放登记。'
      if (result.remembered) message.value += ' 已在本机记住公开设备；恢复始终锁定，不恢复旧授权。'
    }
  } catch (error) {
    if (!stillCurrent(token, attempt)) cleanupFailed()
    else message.value = typeof error === 'object' && error !== null && 'code' in error && error.code === 'REMOTE_HOST_UNAVAILABLE' ? '当前构建未启用远程登记。' : '登记操作未确认成功。请刷新状态，不会自动重试。'
  } finally { if (stillCurrent(token, attempt)) busy.value = false }
}
function show() {
  const t = tabs.tab(props.tabId)
  if (!t || !available.value) return
  captured = captureNativeAttempt(t); open.value = true; message.value = ''
}
async function readScope() {
  const attempt = captured, token = epoch
  if (!attempt || busy.value || !stillCurrent(token, attempt)) return
  busy.value = true; message.value = ''; pairingScope.value = ''
  try {
    const scope = await remoteHostPairingScope({ runId: attempt.runId, generation: attempt.generation })
    if (stillCurrent(token, attempt)) pairingScope.value = JSON.stringify({ session: scope.session, run: scope.run, generation: scope.generation })
  } catch (error) {
    // 只披露固定安全错误码，不显示任意后端消息或异常载荷。
    const code = typeof error === 'object' && error !== null && 'code' in error ? error.code : undefined
    const safeCode = typeof code === 'string' && ['REMOTE_HOST_UNAVAILABLE', 'NATIVE_CANONICAL_UNAVAILABLE', 'RUN_NOT_READY', 'FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE'].includes(code) ? code : undefined
    if (stillCurrent(token, attempt)) message.value = safeCode
      ? `未能读取公开会话范围（${safeCode}），不会自动重试。`
      : '未能读取公开会话范围，不会自动重试。'
  } finally {
    if (stillCurrent(token, attempt)) busy.value = false
  }
}
async function act(kind: 'configure' | 'request' | 'start' | 'cancel' | 'refresh') {
  const attempt = captured, token = epoch
  if (!attempt || busy.value || !stillCurrent(token, attempt)) return
  if (kind === 'configure' && (!candidate.value || new TextEncoder().encode(candidate.value).length > 16384)) { message.value = '候选必须为不超过16KiB的公开配对JSON。'; return }
  const run = { runId: attempt.runId, generation: attempt.generation }
  busy.value = true; message.value = ''
  try {
    if (kind === 'configure') {
      const result = await remoteHostConfigure({ ...run, candidateJson: candidate.value })
      if (!stillCurrent(token, attempt)) { await remoteHostDiscardCandidate({ ...run, receipt: result.receipt }); return }
      receipt = result.receipt
      if (stillCurrent(token, attempt)) { configured.value = true; candidate.value = ''; message.value = '候选已冻结，尚未授权或打开密钥。请在60秒上限内请求本机批准。' }
    } else if (kind === 'cancel' && !requested && receipt) {
      await remoteHostDiscardCandidate({ ...run, receipt })
      if (stillCurrent(token, attempt)) { receipt = null; configured.value = false; state.value = 'Locked'; message.value = '未授权候选已丢弃，可重新编辑。' }
    } else {
      if (kind === 'request') requested = true
      const call = { request: remoteHostRequest, start: remoteHostStart, cancel: remoteHostCancel, refresh: remoteHostCurrent }[kind]
      const result = await call(run)
      if (stillCurrent(token, attempt)) state.value = result.state
    }
  } catch (error) { if (!stillCurrent(token, attempt)) cleanupFailed(); if (stillCurrent(token, attempt)) message.value = typeof error === 'object' && error !== null && 'code' in error && error.code === 'REMOTE_HOST_UNAVAILABLE' ? '当前构建未启用远程Host，无法配置或启动。' : '操作未确认成功，请检查当前状态。不会自动重试。' }
  finally { if (stillCurrent(token, attempt)) busy.value = false }
}
watch(() => [props.tabId, tabs.tab(props.tabId)?.requestId, tabs.tab(props.tabId)?.runId, tabs.tab(props.tabId)?.generation, available.value], () => { if (captured && (!available.value || !matchesNativeAttempt(tabs.tab(props.tabId), captured))) invalidate() })
onUnmounted(invalidate)
</script>
<template>
  <section class="remote-host-panel" aria-label="单次远程会话">
    <AppButton v-if="!open" size="compact" :disabled="!available" @click="show">单次远程连接</AppButton>
    <div v-else class="remote-host-content">
      <strong>可查看并控制当前会话</strong>
      <p>仅本次应用会话；候选冻结后最多60秒，批准不会续期。不会自动启动或重连。</p>
      <AppButton :disabled="busy" @click="readScope">读取公开会话范围</AppButton>
      <p>公开会话范围不是授权，仅用于独立配对配置；不会自动发送到浏览器。更换会话后须重新配置浏览器连接。</p>
      <output v-if="pairingScope" aria-label="公开会话范围">{{ pairingScope }}</output>
      <label>公开配对 JSON（不要填写私钥、密码或令牌）
        <textarea v-model="candidate" aria-label="公开配对JSON" :disabled="configured || busy" maxlength="16384" spellcheck="false" autocomplete="off" />
      </label>
      <BrowserPairingCandidateEditor :scope-json="pairingScope" :disabled="busy" />
      <section class="remembered-devices" aria-label="已记住设备">
        <strong>本机记住的公开设备</strong>
        <p>只保存公开配对材料。安装状态仅为历史记录，不代表无人值守授权。列表始终 Locked；用于当前会话不恢复旧授权，仍须独立批准。不会自动打开密钥、启动或重连。</p>
        <AppButton :disabled="busy" @click="readDevices">刷新已记住设备</AppButton>
        <p v-if="devices && !devices.devices.length">没有已记住的设备。</p>
        <ul v-if="devices?.devices.length">
          <li v-for="device in devices.devices" :key="device.deviceId">
            <output>{{ device.browserOrigin }} · {{ installationLabel(device.installation) }} · Locked</output>
            <output>Relay: {{ device.relayOrigin }}</output>
            <output>Profile: {{ device.profileId }} · Device: {{ device.deviceId }}</output>
            <div class="remote-host-actions">
              <AppButton :disabled="busy || configured || registrationRequested" @click="useDevice('select', device.deviceId)">用于当前会话</AppButton>
              <AppButton variant="danger" :disabled="busy" @click="useDevice('forget', device.deviceId)">忘记设备（本机确认）</AppButton>
            </div>
          </li>
        </ul>
        <AppButton v-if="pendingForget" @click="cancelForget">取消忘记确认</AppButton>
        <p>Forget 需独立本机确认，只删除本机记住记录，不终止独立已批准的连接。要撤销当前会话权限，请使用下方“取消权限”。</p>
      </section>
      <details>
        <summary>首次浏览器登记</summary>
        <p>粘贴仅含公开材料的登记请求，不含 credential、私钥、密码或令牌。先独立核对两端公钥；浏览器必须另有可信 Host pin，不能从此候选自动生成。</p>
        <label>首次登记 JSON
          <textarea v-model="registrationJson" aria-label="首次登记JSON" :disabled="busy || registrationRequested" maxlength="16384" spellcheck="false" autocomplete="off" />
        </label>
        <label class="remember-choice"><input v-model="remember" type="checkbox" aria-label="在本机记住公开设备" :disabled="busy || registrationRequested" />在本机记住公开设备（Remember，可选）</label>
        <p>默认不保存。勾选后仍需独立本机存储确认；不保存私钥或会话批准。JSON 中的 remember 不能代替此勾选。</p>
        <p>首次本机 Yes 只允许使用现有密钥建立一次登记连接，原始总期限120秒。验证后另有浏览器存储确认；均不授予 Host 控制。</p>
        <div class="remote-host-actions">
          <AppButton :disabled="busy || registrationRequested || configured" @click="register('request')">请求登记密钥使用</AppButton>
          <AppButton :disabled="busy || !registrationRequested" @click="register('refresh')">刷新登记状态</AppButton>
          <AppButton :disabled="busy || !registrationRequested" @click="register('cancel')">取消登记</AppButton>
        </div>
        <output aria-label="登记状态">{{ registrationState }}</output>
      </details>
      <div class="remote-host-actions">
        <AppButton :disabled="configured || busy" @click="act('configure')">校验并冻结</AppButton>
        <AppButton :disabled="!configured || busy || state !== 'Locked'" @click="act('request')">请求本机批准</AppButton>
        <AppButton :disabled="busy || state !== 'ApprovalRecorded'" @click="act('start')">启动一次</AppButton>
        <AppButton :disabled="busy" @click="act('refresh')">刷新状态</AppButton>
        <AppButton :disabled="busy" @click="act('cancel')">取消权限</AppButton>
        <AppButton @click="invalidate">关闭</AppButton>
      </div>
      <InlineNotice :message="statusText" />
      <InlineNotice v-if="message" kind="warning" :message="message" />
    </div>

    <ShortRemoteSessionPanel :run="shortRun" :devices="devices" />
  </section>
</template>
<style scoped>
.remote-host-panel { position: absolute; right: 8px; top: 8px; z-index: 5; max-width: min(480px, 95%); color: var(--text-primary); }
.remote-host-content { padding: 12px; border: 1px solid var(--border-color); border-radius: var(--radius-md); background: var(--bg-primary); max-height: 80vh; overflow: auto; }
p, label { font-size: 12px; }
output { display: block; overflow-wrap: anywhere; user-select: text; font-size: 12px; }
textarea { display: block; width: 100%; min-height: 100px; color: var(--text-primary); background: var(--bg-secondary); border: 1px solid var(--border-color); }
.remembered-devices { margin: 12px 0; padding: 8px; border: 1px solid var(--border-color); border-radius: var(--radius-md); }
.remembered-devices ul { list-style: none; padding: 0; }
.remembered-devices li + li { border-top: 1px solid var(--border-color); padding-top: 8px; }
.remember-choice { display: flex; align-items: center; gap: 6px; }
.remote-host-actions { display: flex; flex-wrap: wrap; gap: 6px; margin: 8px 0; }
</style>
