<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { captureNativeAttempt, matchesNativeAttempt, useNativeTabsStore, type NativeAttemptIdentity } from '@/stores/nativeTabs'
import { remoteHostConfigure, remoteHostCurrent, remoteHostRequest, remoteHostStart, remoteHostCancel, remoteHostDiscardCandidate } from '@/api/tauri'
import { useNotificationsStore } from '@/stores/notifications'
const notifications = useNotificationsStore()
function cleanupFailed() { notifications.pushToast({ kind: 'warning', messageKey: 'remoteHostCleanupUnconfirmed' }) }
const props = defineProps<{ tabId: string }>()
const tabs = useNativeTabsStore()
const open = ref(false), candidate = ref(''), busy = ref(false), configured = ref(false)
const state = ref('Locked'), message = ref('')
let epoch = 0, captured: NativeAttemptIdentity | null = null
let receipt: string | null = null, requested = false
const available = computed(() => { const t = tabs.tab(props.tabId); return t?.status === 'running' && t.generation > 0 })
const statusText = computed(() => ({ Locked: '尚无批准', Pending: '等待本机原生确认', ApprovalRecorded: '已记录批准，尚未启动', Consumed: '单次启动已受理，不代表已连接', Registered: '已登记，不代表已连接', Cancelled: '已取消', Expired: '已到期' }[state.value] ?? '状态未知'))
function stillCurrent(token: number, attempt: NativeAttemptIdentity) { return token === epoch && open.value && matchesNativeAttempt(tabs.tab(props.tabId), attempt) }
function invalidate() {
  const previous = captured, previousReceipt = receipt, wasRequested = requested
  epoch++; captured = null; receipt = null; requested = false; candidate.value = ''; configured.value = false; busy.value = false; state.value = 'Locked'; message.value = ''; open.value = false
  if (previous) {
    const run = { runId: previous.runId, generation: previous.generation }
    if (wasRequested) void remoteHostCancel(run).catch(cleanupFailed)
    else if (previousReceipt) void remoteHostDiscardCandidate({ ...run, receipt: previousReceipt }).catch(cleanupFailed)
  }
}
function show() {
  const t = tabs.tab(props.tabId)
  if (!t || !available.value) return
  captured = captureNativeAttempt(t); open.value = true; message.value = ''
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
      <label>公开配对 JSON（不要填写私钥、密码或令牌）
        <textarea v-model="candidate" aria-label="公开配对JSON" :disabled="configured || busy" maxlength="16384" spellcheck="false" autocomplete="off" />
      </label>
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

  </section>
</template>
<style scoped>
.remote-host-panel { position: absolute; right: 8px; top: 8px; z-index: 5; max-width: min(480px, 95%); color: var(--text-primary); }
.remote-host-content { padding: 12px; border: 1px solid var(--border-color); border-radius: var(--radius-md); background: var(--bg-primary); max-height: 80vh; overflow: auto; }
p, label { font-size: 12px; }
textarea { display: block; width: 100%; min-height: 100px; color: var(--text-primary); background: var(--bg-secondary); border: 1px solid var(--border-color); }
.remote-host-actions { display: flex; flex-wrap: wrap; gap: 6px; margin: 8px 0; }
</style>
