<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '../components/ui/AppButton.vue'
import AppDialog from '../components/ui/AppDialog.vue'
import AppSelect from '../components/ui/AppSelect.vue'
import { createVersionManagerClient } from './api'
import type { ManagerMutation, ManagerStatus } from '../types/versionManager'

const { t, locale } = useI18n({ useScope: 'local', messages: {
  en: {
    title: 'Historical version recovery', subtitle: 'CC Desk Version Manager', language: 'Language', theme: 'Appearance',
    system: 'System', light: 'Light', dark: 'Dark', previous: 'Previous version', historical: 'Historical version',
    loading: 'Checking recovery state', pending: 'Request in progress', pendingDetail: 'The result has not been confirmed. Keep this manager open.',
    stale: 'State needs checking', lastKnown: 'Last checked state: {state}', refresh: 'Check again', refreshing: 'Checking…',
    confirm: 'I opened this version and it works', restore: 'Return to previous version', back: 'Back',
    confirmTitle: 'Confirm historical version', confirmReview: 'Confirm only after you have opened CC Desk {version} and checked that it works. This records your confirmation; it does not launch the app. The saved previous version remains available for return.',
    returnTitle: 'Return to CC Desk {version}', returnReview: 'The manager will recheck running processes and the saved files before restoring the previous app and its matching Desk context. Close the historical app and its sessions yourself first. Historical Desk changes will not be merged into the saved previous context. Shared CLI data and project files are not rolled back.',
    restoreSubmit: 'Restore previous version', confirmSubmit: 'Record my confirmation',
    boundaryTitle: 'Your settings and shared data', fresh: 'Fresh settings use a separate Desk context. Preferences, project metadata and GUI state are not carried over. Returning restores the saved matching context; historical Desk changes will not be merged.',
    shared: 'CLI history, credentials/configuration and project files stay shared and are not rolled back. Older provider-era versions can change shared Claude settings. Fresh Desk settings are not a sandbox.',
    handoff: 'The switch is already with the manager. Closing this window does not cancel it. Keep this window open while work is in progress.',
    uncertain: 'A request was already submitted for this state. It will not be repeated. Check again for a newer recovery state before taking another action.',
    checkingFailure: 'The current recovery state could not be checked. Previous details may be stale. Check again before taking an action.',
    actionFailure: 'The request outcome is not confirmed. Check the recovery state; the request will not be repeated automatically.',
    documentFailure: 'This manager document is no longer available. Reopen the version manager to inspect recovery. No action has been sent through another window.',
    startupFailure: 'The manager connection is not ready. Check again. If it remains unavailable, reopen the version manager.',
    invalidFailure: 'The manager returned an unrecognized recovery state. Actions are unavailable. Check again or reopen the version manager.',
    phases: {
      preparing: { title: 'Preparing the version switch', detail: 'The manager is checking and preserving the current installation and Desk context. No installation success is confirmed.' },
      installing: { title: 'Installing historical version', detail: 'The installer is in progress. Installation and first launch are not yet confirmed.' },
      'installed-unconfirmed': { title: 'Installed, awaiting confirmation', detail: 'The historical application files were checked. Its first launch has not been confirmed. Confirm only after you have opened it and checked that it works.' },
      'historical-active': { title: 'Historical version confirmed', detail: 'Your first-launch confirmation was recorded. The previous application and matching Desk context remain available for return.' },
      returning: { title: 'Restoring previous version', detail: 'The manager is preserving the historical context and restoring the previous application and matching Desk context. Return is not yet confirmed.' },
      restored: { title: 'Previous version restored', detail: 'The backend confirmed restoration of the previous application and its matching Desk context. This does not confirm that the application has been opened.' },
      'pre-context-aborted': { title: 'Switch stopped before context change', detail: 'The manager stopped the switch before changing the active Desk context. Check the reported state before reopening the application.' },
      'recovery-required': { title: 'Recovery needs attention', detail: 'The switch has not reached a confirmed safe outcome. Keep the saved contexts and use only the recovery actions offered below.' },
    },
    blocks: {
      SOURCE_STILL_RUNNING: 'The previous CC Desk instance is still running. Close it and its sessions, then check again.',
      SOURCE_EXIT_UNCONFIRMED: 'The previous instance has not been confirmed closed. Wait for it to exit, then check again.',
      SESSIONS_NOT_QUIESCENT: 'Relevant app processes or sessions are still active or unknown. Close them yourself, then check again.',
      INSTALLER_OUTCOME_UNKNOWN: 'The installer outcome is unknown. Check again and use an offered recovery action when available.',
      PAYLOAD_UNVERIFIED: 'The application files have not been verified. An unverified installation cannot be confirmed.',
      PAYLOAD_CHANGED: 'Application files changed after verification. The manager must recheck them before continuing.',
      RETURN_CONFLICT: 'Current files conflict with the saved return context. Keep both contexts; the manager will not silently overwrite the changes.',
      RECOVERY_EVIDENCE_UNAVAILABLE: 'The evidence needed for a safe return is unavailable. Preserve the saved files and check again.',
      STORAGE_UNAVAILABLE: 'Required storage is unavailable. Check free space and access, then check the recovery state again.',
      DOCUMENT_CHANGED: 'The manager document changed. Reopen the version manager to inspect recovery.',
      MANAGER_HANDOFF_INTERRUPTED: 'The handoff to the manager was interrupted. Check recovery before reopening the application.',
    },
  },
  zh: {
    title: '历史版本与恢复', subtitle: 'CC Desk 版本管理器', language: '语言', theme: '外观',
    system: '跟随系统', light: '浅色', dark: '深色', previous: '原版本', historical: '历史版本',
    loading: '正在检查恢复状态', pending: '请求处理中', pendingDetail: '结果尚未确认，请保持版本管理器打开。',
    stale: '需要重新检查状态', lastKnown: '上次检查状态：{state}', refresh: '重新检查', refreshing: '正在检查…',
    confirm: '我已打开此版本并确认可用', restore: '返回原版本', back: '返回',
    confirmTitle: '确认历史版本可用', confirmReview: '仅在已打开 CC Desk {version} 并检查可用后确认。此操作只记录您的确认，不会启动应用。保存的原版本仍可用于返回。',
    returnTitle: '返回 CC Desk {version}', returnReview: '管理器会重新检查运行中的进程和保存的文件，再恢复原应用及其对应的 Desk 数据。请先自行关闭历史应用及其会话。历史版本的 Desk 更改不会合并到保存的原数据中。共享的 CLI 数据和项目文件不会回滚。',
    restoreSubmit: '恢复原版本', confirmSubmit: '记录我的确认',
    boundaryTitle: '设置与共享数据', fresh: '全新设置使用独立的 Desk 数据环境，不继承偏好、项目元数据和界面状态。返回时恢复保存的对应数据，历史版本的 Desk 更改不会合并。',
    shared: 'CLI 历史、凭证/配置和项目文件仍共享，且不会回滚。带有旧 Provider 功能的版本可能更改共享的 Claude 设置。全新 Desk 设置并不是沙箱。',
    handoff: '切换已交由管理器处理。关闭此窗口不代表取消，请在处理期间保持窗口打开。',
    uncertain: '已对此状态提交过请求，不会重复提交。请重新检查，获取更新的恢复状态后再操作。',
    checkingFailure: '无法检查当前恢复状态，之前的详情可能已过期。操作前请重新检查。',
    actionFailure: '请求结果尚未确认。请检查恢复状态，此请求不会自动重复提交。',
    documentFailure: '此管理器页面已不可用，请重新打开版本管理器检查恢复状态。未通过其他窗口发送操作。',
    startupFailure: '管理器连接尚未就绪，请重新检查。如果仍不可用，请重新打开版本管理器。',
    invalidFailure: '管理器返回了无法识别的恢复状态，操作暂不可用。请重新检查或重新打开版本管理器。',
    phases: {
      preparing: { title: '正在准备版本切换', detail: '管理器正在检查并保存当前安装及 Desk 数据，尚未确认安装成功。' },
      installing: { title: '正在安装历史版本', detail: '安装程序正在处理，安装结果和首次启动尚未确认。' },
      'installed-unconfirmed': { title: '已安装，等待确认可用', detail: '历史应用文件已检查，但首次启动尚未确认。请在打开历史版本并检查可用后再确认。' },
      'historical-active': { title: '历史版本已确认可用', detail: '已记录您的首次启动确认，原应用及对应的 Desk 数据仍保留，可用于返回。' },
      returning: { title: '正在恢复原版本', detail: '管理器正在保留历史版本数据，并恢复原应用及对应的 Desk 数据。返回结果尚未确认。' },
      restored: { title: '原版本已恢复', detail: '后端已确认原应用及其对应 Desk 数据恢复完成，这不代表应用已打开。' },
      'pre-context-aborted': { title: '切换在数据变更前停止', detail: '管理器在更改当前 Desk 数据前停止了切换。重新打开应用前请检查状态。' },
      'recovery-required': { title: '恢复需要处理', detail: '尚未确认切换已到达安全结果。请保留保存的数据，仅使用下方提供的恢复操作。' },
    },
    blocks: {
      SOURCE_STILL_RUNNING: '原 CC Desk 实例仍在运行。请关闭它及其会话后重新检查。',
      SOURCE_EXIT_UNCONFIRMED: '尚未确认原实例已关闭。请等待它退出后重新检查。',
      SESSIONS_NOT_QUIESCENT: '相关应用进程或会话仍在运行，或状态未知。请自行关闭后重新检查。',
      INSTALLER_OUTCOME_UNKNOWN: '安装程序结果未知。请重新检查，并在恢复操作可用时使用。',
      PAYLOAD_UNVERIFIED: '应用文件尚未验证，不能确认未验证的安装可用。',
      PAYLOAD_CHANGED: '应用文件在验证后发生更改，管理器必须重新检查后才能继续。',
      RETURN_CONFLICT: '当前文件与保存的返回数据冲突。请保留两份数据，管理器不会静默覆盖更改。',
      RECOVERY_EVIDENCE_UNAVAILABLE: '安全返回所需的证据不可用，请保留保存的文件并重新检查。',
      STORAGE_UNAVAILABLE: '所需存储不可用。请检查可用空间和访问权限，再检查恢复状态。',
      DOCUMENT_CHANGED: '管理器页面已更改，请重新打开版本管理器检查恢复状态。',
      MANAGER_HANDOFF_INTERRUPTED: '向管理器的交接已中断，重新打开应用前请检查恢复状态。',
    },
  },
} })

const client = createVersionManagerClient()
const status = shallowRef<ManagerStatus | null>(null)
const review = shallowRef<{ action: ManagerMutation; status: ManagerStatus } | null>(null)
const busy = ref<'inspect' | 'action' | null>(null)
const fresh = ref(false)
const errorKey = ref('')
const theme = ref('system')
const statusHeading = ref<HTMLElement | null>(null)
const systemTheme = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null
let alive = false
let sequence = 0
let startupAttempts = 0
let timer: ReturnType<typeof setTimeout> | undefined
const offered = computed(() => fresh.value && client?.isCurrent() ? status.value?.allowedActions ?? [] : [])
const uncertain = computed(() => !!status.value && offered.value.some(action => action !== 'refresh')
  && !offered.value.some(action => action !== 'refresh' && client?.canAct(action, status.value!)))
const title = computed(() => busy.value === 'action' ? t('pending') : errorKey.value ? t('stale')
  : status.value ? t(`phases.${status.value.phase}.title`) : t('loading'))
function allowed(action: ManagerMutation) {
  return !busy.value && fresh.value && !!status.value && !!client?.canAct(action, status.value)
}
function codeOf(error: unknown): string {
  if (error instanceof Error) return error.message
  if (error && typeof error === 'object' && 'code' in error && typeof error.code === 'string') return error.code
  return ''
}
function clearTimer() { if (timer !== undefined) { clearTimeout(timer); timer = undefined } }
function scheduleRefresh() {
  clearTimer()
  if (alive && fresh.value && status.value && ['preparing', 'installing', 'returning'].includes(status.value.phase)) {
    timer = setTimeout(() => { void refresh() }, 1500)
  }
}
async function refresh() {
  if (!alive || busy.value) return
  clearTimer()
  review.value = null
  if (!client?.isCurrent()) { fresh.value = false; errorKey.value = 'documentFailure'; return }
  const request = ++sequence
  busy.value = 'inspect'; errorKey.value = ''; fresh.value = false
  try {
    const next = await client.inspect()
    if (!alive || request !== sequence) return
    status.value = next; fresh.value = true
  } catch (error) {
    if (!alive || request !== sequence) return
    const code = codeOf(error)
    if (code === 'FORBIDDEN' && !status.value && client.isCurrent()) {
      errorKey.value = 'startupFailure'
      if (++startupAttempts < 4) timer = setTimeout(() => { void refresh() }, 200)
    } else errorKey.value = !client.isCurrent() || ['MANAGER_DOCUMENT_CHANGED', 'DOCUMENT_BRIDGE_UNAVAILABLE'].includes(code)
      ? 'documentFailure' : code === 'MANAGER_INVALID_RESPONSE' ? 'invalidFailure' : 'checkingFailure'
  } finally {
    if (alive && request === sequence) { busy.value = null; if (fresh.value) scheduleRefresh() }
  }
}
function openReview(action: ManagerMutation) {
  if (status.value && allowed(action)) review.value = { action, status: status.value }
}
async function submitReview() {
  const intent = review.value
  if (!intent || !client || intent.status !== status.value || !allowed(intent.action)) { review.value = null; return }
  review.value = null
  clearTimer()
  const request = ++sequence
  busy.value = 'action'; fresh.value = false; errorKey.value = ''
  await nextTick()
  // The shared dialog restores only to an available opener; this stable status
  // heading gives keyboard users a destination after the submitted action leaves.
  if (!alive || request !== sequence) return
  statusHeading.value?.focus()
  try {
    const next = await client.act(intent.action, intent.status)
    if (!alive || request !== sequence) return
    status.value = next; fresh.value = true
  } catch (error) {
    if (!alive || request !== sequence) return
    errorKey.value = !client.isCurrent() || codeOf(error) === 'MANAGER_DOCUMENT_CHANGED' ? 'documentFailure' : 'actionFailure'
  } finally {
    if (alive && request === sequence) { busy.value = null; scheduleRefresh() }
  }
}
function applyTheme() {
  document.documentElement.dataset.theme = theme.value === 'system' ? systemTheme?.matches ? 'dark' : 'light' : theme.value
}
watch(theme, applyTheme, { immediate: true })
watch(locale, value => { document.documentElement.lang = value === 'zh' ? 'zh-CN' : 'en' }, { immediate: true })
onMounted(() => { alive = true; systemTheme?.addEventListener('change', applyTheme); void refresh() })
onBeforeUnmount(() => { alive = false; ++sequence; clearTimer(); systemTheme?.removeEventListener('change', applyTheme) })
</script>

<template>
  <main class="version-manager" :aria-busy="busy === 'action' || undefined">
    <header class="manager-header">
      <div><p class="manager-eyebrow">{{ t('subtitle') }}</p><h1>{{ t('title') }}</h1></div>
      <div class="manager-preferences">
        <AppSelect v-model="locale" data-manager-language :label="t('language')" :options="[{ value: 'en', label: 'English' }, { value: 'zh', label: '简体中文' }]" size="compact" />
        <AppSelect v-model="theme" data-manager-theme :label="t('theme')" :options="[{ value: 'system', label: t('system') }, { value: 'light', label: t('light') }, { value: 'dark', label: t('dark') }]" size="compact" />
      </div>
    </header>
    <div class="manager-content">
      <dl v-if="status" class="manager-versions">
        <div><dt>{{ t('previous') }}</dt><dd>{{ status.sourceVersion }}</dd></div>
        <div><dt>{{ t('historical') }}</dt><dd>{{ status.targetVersion }}</dd></div>
      </dl>
      <section class="manager-status" aria-labelledby="manager-status-title" :data-phase="fresh ? status?.phase : 'unknown'">
        <h2 id="manager-status-title" ref="statusHeading" tabindex="-1" data-manager-phase role="status" aria-live="polite" aria-atomic="true">{{ title }}</h2>
        <p v-if="busy === 'action'">{{ t('pendingDetail') }}</p>
        <p v-else-if="status && fresh">{{ t(`phases.${status.phase}.detail`) }}</p>
        <p v-else-if="status">{{ t('lastKnown', { state: t(`phases.${status.phase}.title`) }) }}</p>
        <p v-if="errorKey" class="manager-warning" data-manager-error role="alert">{{ t(errorKey) }}</p>
        <p v-if="fresh && status?.blockedReason" class="manager-warning" data-manager-block>{{ t(`blocks.${status.blockedReason}`) }}</p>
        <p v-if="uncertain && !busy" class="manager-warning" data-manager-uncertain>{{ t('uncertain') }}</p>
      </section>
      <section class="manager-boundary" data-manager-boundary aria-labelledby="manager-boundary-title">
        <h2 id="manager-boundary-title">{{ t('boundaryTitle') }}</h2>
        <p>{{ t('fresh') }}</p><p>{{ t('shared') }}</p>
      </section>
      <p v-if="status && !['restored', 'pre-context-aborted'].includes(status.phase)" class="manager-handoff">{{ t('handoff') }}</p>
    </div>
    <footer class="manager-actions">
      <AppButton data-manager-refresh :disabled="!!busy || !client?.isCurrent()" :loading="busy === 'inspect'" @click="refresh">{{ t(busy === 'inspect' ? 'refreshing' : 'refresh') }}</AppButton>
      <AppButton v-if="offered.includes('confirm-historical-version')" data-manager-confirm variant="primary" :disabled="!allowed('confirm-historical-version')" @click="openReview('confirm-historical-version')">{{ t('confirm') }}</AppButton>
      <AppButton v-if="offered.includes('return-to-previous')" data-manager-return variant="danger" :disabled="!allowed('return-to-previous')" @click="openReview('return-to-previous')">{{ t('restore') }}</AppButton>
    </footer>
    <AppDialog :open="!!review" :show-close="false" :title="review?.action === 'return-to-previous' ? t('returnTitle', { version: review.status.sourceVersion }) : t('confirmTitle')" @update:open="open => { if (!open) review = null }">
      <p class="ui-description">{{ review?.action === 'return-to-previous' ? t('returnReview') : t('confirmReview', { version: review?.status.targetVersion ?? '' }) }}</p>
      <template #footer>
        <AppButton data-manager-back autofocus @click="review = null">{{ t('back') }}</AppButton>
        <AppButton data-manager-submit :variant="review?.action === 'return-to-previous' ? 'danger' : 'primary'" :disabled="!review || !allowed(review.action)" @click="submitReview">{{ t(review?.action === 'return-to-previous' ? 'restoreSubmit' : 'confirmSubmit') }}</AppButton>
      </template>
    </AppDialog>
  </main>
</template>

<style scoped>
.version-manager { display: flex; flex-direction: column; height: 100vh; min-width: 0; background: var(--bg-primary); }
.manager-header { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 16px; padding: 22px 28px 18px; border-bottom: 1px solid var(--border-light); }
.manager-eyebrow { font-size: 12px; color: var(--text-secondary); margin-bottom: 4px; }
h1 { font-size: 22px; line-height: 1.3; font-weight: 600; }
.manager-preferences { display: flex; gap: 10px; }
.manager-preferences :deep(.ui-field) { min-width: 102px; }
.manager-content { min-height: 0; overflow-y: auto; padding: 24px 28px; display: grid; gap: 22px; align-content: start; }
.manager-versions { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; border-bottom: 1px solid var(--border-light); padding-bottom: 20px; }
.manager-versions dt { font-size: 12px; color: var(--text-secondary); }
.manager-versions dd { font-size: 22px; font-weight: 600; margin-top: 4px; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.manager-status { padding-inline-start: 16px; border-inline-start: 3px solid var(--accent-gold); display: grid; gap: 10px; }
.manager-status h2 { font-size: 18px; font-weight: 600; outline-offset: 4px; }
.manager-status h2:focus-visible { outline: 2px solid var(--focus-ring); }
.manager-status p, .manager-boundary p { font-size: 13px; line-height: 1.6; color: var(--text-secondary); overflow-wrap: anywhere; }
.manager-status .manager-warning { color: var(--text-primary); padding: 10px 12px; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--radius-md); }
.manager-boundary { display: grid; gap: 8px; }
.manager-boundary h2 { font-size: 14px; font-weight: 600; }
.manager-handoff { color: var(--text-secondary); font-size: 12px; line-height: 1.6; }
.manager-actions { flex-shrink: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 16px 28px; border-top: 1px solid var(--border-light); }
.manager-actions :deep(.ui-button) { white-space: normal; height: auto; min-height: 36px; padding-top: 7px; padding-bottom: 7px; line-height: 1.4; max-width: 100%; }
@media (max-width: 680px) { .manager-header { padding: 18px 20px; } .manager-content { padding: 20px; } .manager-actions { padding: 14px 20px; } h1 { font-size: 20px; } }
</style>
