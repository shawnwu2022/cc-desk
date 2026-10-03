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
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n(); const app = useAppStore(), notifications = useNotificationsStore(), counts = useOwnedSessionCounts()
const version = __APP_VERSION__, buildCommit = /^[0-9a-f]{40}$/.test(__APP_BUILD_COMMIT__) ? __APP_BUILD_COMMIT__ : 'unknown'
const links = [
  { id: 'github', label: 'githubRepo', url: 'https://github.com/shawnwu2022/cc-desk' },
  { id: 'claude', label: 'claudeDocs', url: 'https://code.claude.com/docs' },
  { id: 'codex', label: 'codexDocs', url: 'https://developers.openai.com/learn/codex' },
  { id: 'license', label: 'aboutLicense', url: 'https://github.com/shawnwu2022/cc-desk#license' },
]
const error = ref<string | null>(null), copying = ref(false)
let owner = 0
function invalidate() { ++owner; error.value = null; copying.value = false }
watch(() => props.active, active => { if (!active) invalidate() }, { flush: 'sync' }); onBeforeUnmount(invalidate)
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
  const summary = safeAppDiagnostics({ version, commit: buildCommit, platform,
    gui: { mode: app.guiThemeMode, density: app.guiDensity, sidebarWidth: app.sidebarWidth },
    terminal: { theme: app.terminalTheme, font: app.terminalFontFamily, size: app.fontSize, lineHeight: app.terminalLineHeight,
      cursor: app.terminalCursorStyle, blink: app.terminalCursorBlink, renderer: app.webglRenderer }, sessions: counts.value })
  try {
    await writeText(JSON.stringify(summary, null, 2))
    if (versionOwner === owner && props.active) notifications.pushToast({ kind: 'success', messageKey: 'diagnosticsCopied' })
  } catch { if (versionOwner === owner && props.active) error.value = 'diagnosticsCopyFailed' }
  finally { if (versionOwner === owner) copying.value = false }
}
</script>
<template>
  <section class="remaining-settings" data-settings-about>
    <h2>{{ t('aboutTitle') }}</h2>
    <div class="about-card"><img src="@/assets/icons/app-icon.png" alt="CC Desk" /><div><strong>CC Desk</strong><p>{{ t('version', { version }) }}</p><p>{{ t('aboutDesc') }}</p></div></div>
    <p data-build-commit class="build-commit">{{ t('aboutBuildCommit') }}: {{ buildCommit === 'unknown' ? t('aboutBuildUnknown') : buildCommit }}</p>
    <p>MIT · {{ t('aboutLicense') }}</p>
    <div class="about-links"><AppButton v-for="link in links" :key="link.id" :data-about-link="link.id" variant="ghost" @click="external(link.id)">{{ t(link.label) }} ↗</AppButton></div>
    <p class="settings-hint">{{ t('diagnosticsPrivacyHint') }}</p>
    <AppButton data-copy-diagnostics :disabled="copying || !active" @click="copyDiagnostics">{{ t('copyDiagnostics') }}</AppButton>
    <InlineNotice v-if="error" kind="warning" :message="t(error)" />
  </section>
</template>
<style scoped>
.remaining-settings { display: flex; flex-direction: column; align-items: flex-start; gap: 16px; min-width: 0; max-width: 760px; color: var(--text-primary); }
h2 { font-size: 20px; } .about-card { display: flex; gap: 16px; align-items: center; min-width: 0; } .about-card img { width: 56px; height: 56px; flex-shrink: 0; }
.about-card p, .build-commit, .settings-hint { font-size: 12px; line-height: 1.6; color: var(--text-secondary); overflow-wrap: anywhere; }
.about-links { display: flex; gap: 8px; flex-wrap: wrap; min-width: 0; } .about-links :deep(button) { white-space: normal; }
</style>
