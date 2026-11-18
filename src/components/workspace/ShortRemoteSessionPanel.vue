<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import AppButton from '@/components/ui/AppButton.vue'
import { useNotificationsStore } from '@/stores/notifications'
import { remoteShortRequest, remoteReconnectRequest, remoteShortAdvance, remoteShortCurrent, remoteShortClose, type RemotePairingList, type RemoteHostRun, type RemoteShortReply } from '@/api/tauri'
const props = defineProps<{ run: RemoteHostRun|null; devices: RemotePairingList|null }>()
const notifications = useNotificationsStore()
function cleanupFailed() { notifications.pushToast({ kind:'warning', messageKey:'remoteHostCleanupUnconfirmed' }) }
const attempted = ref(false)
const selected = ref(''), busy = ref(false), message = ref('')
const reply = ref<RemoteShortReply>({ phase: 'empty', operationId: null, limitSeconds: 10 })
let disposed = false, cancelled = false, timer: ReturnType<typeof setTimeout>|undefined
const labels = { empty:'默认关闭', preparing:'等待本机准备确认', confirming:'等待独立短网络确认', starting:'正在启动，尚未确认连接', accepted:'启动已受理，不代表手机已连接', closing:'已禁止新操作，正在收口', closed:'已关闭；本轮不支持再次开启', unknown:'收口未确认，请勿重试' }
const label = computed(() => labels[reply.value.phase])
const canStart = computed(() => !attempted.value && !busy.value && reply.value.phase === 'empty' && props.run && props.devices?.devices.some(d => d.deviceId === selected.value && d.installation === 'confirmed'))
function stopTimer() { if (timer) clearTimeout(timer); timer = undefined }
async function discardLate(value: RemoteShortReply) { if(value.operationId) await remoteShortClose(value.operationId) }
function schedule() {
  stopTimer()
  if (!disposed && !cancelled && ['preparing','confirming','starting','accepted','closing'].includes(reply.value.phase)) timer=setTimeout(() => { void progress() }, 200)
}
async function progress() {
  if (disposed || busy.value || cancelled) return
  busy.value=true
  try {
    const before=reply.value
    const value=before.operationId && ['preparing','confirming'].includes(before.phase)
      ? await remoteShortAdvance(before.operationId) : await remoteShortCurrent()
    if (disposed || cancelled) { await discardLate(value); return }
    reply.value=value
  } catch { if(disposed) cleanupFailed(); message.value='状态未确认。已停止自动推进，请刷新或关闭，不会自动重试启动。'; return }
  finally { busy.value=false }
  schedule()
}
async function start(reconnect = false) {
  if(!canStart.value || !props.run || !props.devices) return
  const run={...props.run}, list=props.devices, deviceId=selected.value
  attempted.value=true; cancelled=false; busy.value=true; message.value=''
  try {
    const request = reconnect ? remoteReconnectRequest : remoteShortRequest
    const value=await request({...run,candidateJson:JSON.stringify({storeId:list.storeId,revision:list.revision,deviceId})})
    if(disposed || cancelled) { await discardLate(value); return }
    reply.value=value
  } catch { if(disposed) cleanupFailed(); message.value='短会话未确认开启。本次App不再重试；请刷新状态核对。不会生成设备、修复存储或自动重试。'; return }
  finally { busy.value=false }
  schedule()
}
async function refresh() {
  if(busy.value) return
  busy.value=true
  try { reply.value=await remoteShortCurrent() }
  catch { message.value='无法确认当前状态。请勿把读取失败当作已关闭。' }
  finally { busy.value=false }
}
async function close() {
  cancelled=true; stopTimer(); message.value='已请求关闭，等待原资源收口。'
  try {
    const current=reply.value.operationId ? reply.value : await remoteShortCurrent()
    if(current.operationId) { await remoteShortClose(current.operationId); reply.value={...current,phase:'closing'} }
    else message.value='尚无已登记操作；若请求仍在返回途中，将在返回后立即取消。'
  } catch { message.value='关闭未确认。不会继续推进，请刷新状态后核对。' }
}
onUnmounted(() => { disposed=true; stopTimer(); if(['preparing','confirming'].includes(reply.value.phase)) {cancelled=true; void discardLate(reply.value).catch(cleanupFailed)} })
</script>
<template>
  <details class="short-session" aria-label="实验短会话">
    <summary>远程试用入口（默认关闭）</summary>
    <p>旧10秒入口：默认关闭。仅当前精确会话与已确认设备；最终确认前开始计时，确认和连接均占用这10秒。不会重连、延时或跨重启恢复，不是长期无人值守。</p>
    <p>先在上方完成真实浏览器登记并刷新设备；不会自动生成手机身份或配置密钥。</p>
    <label>已确认设备
      <select v-model="selected" aria-label="短会话设备" :disabled="busy || reply.phase !== 'empty'">
        <option value="">请选择设备</option>
        <option v-for="device in devices?.devices ?? []" :key="device.deviceId" :value="device.deviceId" :disabled="device.installation !== 'confirmed'">{{ device.browserOrigin }} · {{ device.deviceId }}{{ device.installation === 'confirmed' ? '' : '（安装尚未确认）' }}</option>
      </select>
    </label>
    <div class="short-actions">
      <AppButton :disabled="!canStart" @click="start(false)">请求10秒短会话</AppButton>
      <AppButton data-reconnect-request :disabled="!canStart" @click="start(true)">请求最多60秒可重连试用</AppButton>
      <AppButton :disabled="busy" @click="refresh">刷新短会话状态</AppButton>
      <AppButton variant="danger" @click="close">关闭短会话</AppButton>
    </div>
    <p>可重连试用需另行本机明确批准，仅同一设备与当前会话，最多60秒；确认时间计入截止。Host无客户端等待最多替代3次，浏览器安全空闲重试另有最多3次预算，均不保证连接成功或长期无人值守。</p>
    <p v-if="reply.mode === 'reconnectTrial'">当前为可重连试用。实际截止：{{ reply.deadlineUnixMs == null ? '尚未冻结，以最终确认显示为准' : new Date(reply.deadlineUnixMs).toLocaleString() }}。</p>
    <p v-if="reply.limitSeconds == null">当前授权详情未知，不推定10秒或60秒许可。</p>
    <output aria-label="短会话状态">{{ label }}</output>
    <p v-if="message" role="status">{{ message }}</p>
    <p>关闭与状态读取不要求旧会话仍在运行。已有持久记录不会被重置；此版本不提供重复开启。</p>
  </details>
</template>
<style scoped>
.short-session { padding:8px; background:var(--bg-primary); border:1px solid var(--border-color); border-radius:var(--radius-md); }
.short-actions { display:flex; flex-wrap:wrap; gap:6px; margin:8px 0; }
p,label,output { font-size:12px; }
select { width:100%; color:var(--text-primary); background:var(--bg-secondary); }
</style>
