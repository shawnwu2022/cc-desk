<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { useNotificationsStore } from '@/stores/notifications'
import { useOwnedSessionCounts } from '@/composables/useOwnedSessionCounts'
import { platform } from '@/utils/platform'
import { safeAppDiagnostics } from '@/utils/appDiagnostics'
import { safeUpdateDiagnostics } from '@/utils/updateDiagnostics'
import { useUpdateStore } from '@/stores/update'
import { getUpdaterSettings } from '@/api/tauri'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliSupportStore } from '@/stores/cliSupport'
import { useCliVersionDiagnostics } from '@/composables/useCliVersionDiagnostics'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n(); const app = useAppStore(), updates = useUpdateStore(), notifications = useNotificationsStore(), counts = useOwnedSessionCounts()
const version = __APP_VERSION__, buildCommit = /^[0-9a-f]{40}$/.test(__APP_BUILD_COMMIT__) ? __APP_BUILD_COMMIT__ : 'unknown'
const links = [
  { id: 'github', label: 'githubRepo', url: 'https://github.com/shawnwu2022/cc-desk' },
  { id: 'claude', label: 'claudeDocs', url: 'https://code.claude.com/docs' },
  { id: 'codex', label: 'codexDocs', url: 'https://developers.openai.com/learn/codex' },
  { id: 'opencode', label: 'opencodeDocs', url: 'https://opencode.ai/docs' },
  { id: 'license', label: 'aboutLicense', url: 'https://github.com/shawnwu2022/cc-desk#license' },
]
const error = ref<string | null>(null), copying = ref(false)
const profiles = useCliProfilesStore(), support = useCliSupportStore(), loadingProfiles = ref(false)
const versions = useCliVersionDiagnostics(() => (['claude', 'codex', 'opencode'] as const).flatMap(cli => {
  const profile = profiles.selected[cli]
  return support.isEnabled(cli) && profile ? [{ cli, id: profile.id, revision: profile.revision, name: profile.name }] : []
}), () => props.active)
let collectionOwner = 0
let owner = 0
function invalidate() { ++owner; ++collectionOwner; error.value = null; copying.value = false; loadingProfiles.value = false }
watch(() => props.active, active => { if (!active) invalidate() }, { flush: 'sync' }); onBeforeUnmount(invalidate)
async function collectVersions() {
  if (!props.active || loadingProfiles.value || versions.busy.value) return
  const current = ++collectionOwner; loadingProfiles.value = true; error.value = null
  versions.invalidate()
  try {
    await profiles.load()
    if (current === collectionOwner && props.active) await versions.collect()
  } catch { if (current === collectionOwner && props.active) { versions.invalidate('unavailable'); error.value = 'diagnosticsProfilesUnavailable' } }
  finally { if (current === collectionOwner) loadingProfiles.value = false }
}
function cancelVersions() { ++collectionOwner; loadingProfiles.value = false; void versions.cancel() }
async function external(id: string) {
  const link = links.find(item => item.id === id)
  if (!props.active || !link) return
  const version = owner
  try { await open(link.url) }
  catch { if (version === owner && props.active) error.value = 'settingsExternalLinkFailed' }
}
async function copyDiagnostics() {
  if (!props.active || copying.value) return
  const versionOwner = ++owner; copying.value = true; error.value = null
  const appSummary = safeAppDiagnostics({ version, commit: buildCommit, platform,
    gui: { mode: app.guiThemeMode, density: app.guiDensity, sidebarWidth: app.sidebarWidth },
    terminal: { theme: app.terminalTheme, font: app.terminalFontFamily, size: app.fontSize, lineHeight: app.terminalLineHeight,
      cursor: app.terminalCursorStyle, blink: app.terminalCursorBlink, renderer: app.webglRenderer }, sessions: counts.value })
  try {
    let channel: unknown = null
    try { channel = (await getUpdaterSettings()).channel } catch { /* 未取得配置时报告未知，不猜测所选通道。 */ }
    if (versionOwner !== owner || !props.active) return
    const summary = { ...appSummary, update: safeUpdateDiagnostics({ channel, state: updates.checking ? 'checking' : updates.downloadState,
      candidate: updates.updateInfo?.version, failure: updates.lastFailure, cliVersions: versions.summary.value }) }
    await writeText(JSON.stringify(summary, null, 2))
    if (versionOwner === owner && props.active) notifications.pushToast({ kind: 'success', messageKey: 'diagnosticsCopied' })
  } catch { if (versionOwner === owner && props.active) error.value = 'diagnosticsCopyFailed' }
  finally { if (versionOwner === owner) copying.value = false }
}
</script>
<template>
  <section class="remaining-settings" data-settings-about>
    <h2>{{ t('aboutTitle') }}</h2>
    <div class="about-card"><img class="app-icon" src="@/assets/icons/app-icon.png" alt="CC Desk" /><div><strong>CC Desk</strong><p>{{ t('version', { version }) }}</p><p>{{ t('aboutDesc') }}</p></div></div>
    <p data-build-commit class="build-commit">{{ t('aboutBuildCommit') }}: {{ buildCommit === 'unknown' ? t('aboutBuildUnknown') : buildCommit }}</p>
    <p>MIT · {{ t('aboutLicense') }}</p>
    <div class="about-links"><AppButton v-for="link in links" :key="link.id" :data-about-link="link.id" variant="ghost" @click="external(link.id)">{{ t(link.label) }} ↗</AppButton></div>
    <p class="settings-hint">{{ t('diagnosticsPrivacyHint') }}</p>
    <div class="version-diagnostics" data-cli-version-diagnostics>
      <h3>{{ t('diagnosticsCliTitle') }}</h3>
      <p class="settings-hint">{{ t('diagnosticsCliHint') }}</p>
      <ul v-if="versions.rows.value.length" aria-live="polite">
        <li v-for="row in versions.rows.value" :key="row.cli" :data-cli-version="row.cli">
          <strong>{{ row.cli }}</strong> · {{ row.name }}:
          {{ row.state === 'known' ? row.version : t(`diagnosticsVersionState_${row.state}`) }}
          <span v-if="row.reason"> — {{ t(`diagnosticsVersionReason_${row.reason}`) }}</span>
        </li>
      </ul>
      <p v-else class="settings-hint">{{ t('diagnosticsCliNoSelection') }}</p>
      <div class="about-links">
        <AppButton data-collect-cli-versions :disabled="!active || loadingProfiles || versions.busy.value" @click="collectVersions">{{ t('diagnosticsCollectCli') }}</AppButton>
        <AppButton v-if="loadingProfiles || versions.busy.value" data-cancel-cli-versions variant="ghost" @click="cancelVersions">{{ t('diagnosticsCancelCli') }}</AppButton>
      </div>
    </div>
    <AppButton data-copy-diagnostics :disabled="copying || !active" @click="copyDiagnostics">{{ t('copyDiagnostics') }}</AppButton>
    <InlineNotice v-if="error" kind="warning" :message="t(error)" />
  </section>
</template>
<style scoped>
.remaining-settings { display: flex; flex-direction: column; align-items: flex-start; gap: 16px; min-width: 0; max-width: 760px; color: var(--text-primary); }
h2 { font-size: 20px; } .about-card { display: flex; gap: 16px; align-items: center; min-width: 0; } .about-card img { width: 56px; height: 56px; flex-shrink: 0; }
.about-card p, .build-commit, .settings-hint { font-size: 12px; line-height: 1.6; color: var(--text-secondary); overflow-wrap: anywhere; }
.about-links { display: flex; gap: 8px; flex-wrap: wrap; min-width: 0; } .about-links :deep(button) { white-space: normal; }
.version-diagnostics { display: flex; flex-direction: column; gap: 8px; max-width: 100%; }
.version-diagnostics h3 { font-size: 14px; } .version-diagnostics ul { padding-left: 20px; font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }
</style>
