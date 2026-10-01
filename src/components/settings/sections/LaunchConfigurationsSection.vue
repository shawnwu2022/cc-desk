<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { mapSafeUserError, type UserErrorPresentation } from '@/utils/userError'
import type { LaunchConfigurationEditorRequest } from '@/types/profile'
import AppButton from '@/components/ui/AppButton.vue'
import IconButton from '@/components/ui/IconButton.vue'
import AppMenu from '@/components/ui/AppMenu.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import LoadingState from '@/components/ui/LoadingState.vue'
import CliAppIcon from '@/components/sessions/CliAppIcon.vue'
import LaunchConfigurationEditor from '../LaunchConfigurationEditor.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n()
const profiles = useCliProfilesStore(), defaults = useNewSessionDraftStore()
const editor = ref<LaunchConfigurationEditorRequest | null>(null)
const editorVersion = ref(0), menuId = ref<string | null>(null)
const anchor = ref({ left: '8px', top: '8px' })
const loadError = ref<UserErrorPresentation | null>(null)
let activeOwner = 0, disposed = false
const tools = ['claude', 'codex'] as const
async function load() {
  const owner = ++activeOwner; loadError.value = null
  try { await profiles.load() } catch { if (!disposed && props.active && owner === activeOwner) loadError.value = mapSafeUserError(profiles.lastError ?? 'GENERIC_UNAVAILABLE', 'settings') }
}
function closeSurfaces() { ++activeOwner; editor.value = null; menuId.value = null; profiles.closeDeleteConfirmation(); loadError.value = null }
watch(() => props.active, active => { if (!active) closeSurfaces(); else if (profiles.status === 'idle' || profiles.status === 'error') void load() }, { immediate: true, flush: 'sync' })
onBeforeUnmount(() => { disposed = true; closeSurfaces() })
function open(request: LaunchConfigurationEditorRequest) { if (!props.active) return; profiles.closeDeleteConfirmation(); editorVersion.value++; editor.value = request; menuId.value = null }
function showMenu(id: string, event: MouseEvent) {
  if (!props.active) return
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
  anchor.value = { left: `${Math.max(8, Math.min(event.type === 'contextmenu' ? event.clientX : rect.left, window.innerWidth - 212))}px`, top: `${Math.max(8, Math.min(event.type === 'contextmenu' ? event.clientY : rect.bottom, window.innerHeight - 180))}px` }
  menuId.value = id
}
const menuItems = computed(() => {
  const row = menuId.value ? profiles.profile(menuId.value) : undefined
  return row ? [
    { id: 'copy', label: t('launchConfigCopy') }, { id: 'rename', label: t('launchConfigRename') },
    { id: 'default', label: t('launchConfigMakeDefault'), hidden: defaults.defaultFor(row.cli as 'claude' | 'codex')?.id === row.id },
    { id: 'delete', label: t('delete'), danger: true },
  ] : []
})
function action(value: string) {
  const row = menuId.value ? profiles.profile(menuId.value) : undefined
  menuId.value = null
  if (!props.active || !row || row.cli === 'shell') return
  if (value === 'copy' || value === 'rename') open({ kind: value, profileId: row.id })
  else if (value === 'default') defaults.setDefault(row.cli, row.id)
  else if (value === 'delete') { editor.value = null; profiles.requestDelete(row.id) }
}
</script>
<template>
  <section class="launch-configurations" :aria-label="t('settingsLaunchConfigurations')">
    <header><h2>{{ t('settingsLaunchConfigurations') }}</h2><AppButton variant="ghost" :disabled="profiles.status === 'loading'" @click="load">{{ t('refresh') }}</AppButton></header>
    <p class="launch-hint">{{ t('launchConfigFutureOnly') }}</p>
    <InlineNotice v-if="loadError" :kind="loadError.severity" :message="t(loadError.messageKey)" :action-label="t('retry')" @action="load" />
    <InlineNotice v-if="profiles.deleteError && !profiles.deleteConfirmation" :kind="profiles.deleteError.severity" :message="t(profiles.deleteError.messageKey)" />
    <LoadingState v-if="profiles.status === 'loading' && !profiles.profiles.length" :label="t('loading')" />
    <section v-for="cli in tools" :key="cli" :data-launch-group="cli" class="launch-group">
      <header><h3><CliAppIcon :cli="cli" />{{ cli === 'claude' ? 'Claude Code' : 'Codex CLI' }}</h3><AppButton data-launch-create @click="open({ kind: 'create', cli })">{{ t('launchConfigCreate') }}</AppButton></header>
      <div v-for="row in profiles.byCli[cli]" :key="row.id" :data-launch-row="row.id" class="launch-row" @contextmenu.prevent="showMenu(row.id, $event)">
        <CliAppIcon :cli="cli" /><span class="launch-name">{{ row.name }}</span>
        <span class="launch-default" v-if="defaults.defaultFor(cli)?.id === row.id">{{ t('launchConfigDefault') }}</span><span v-else />
        <AppButton data-launch-edit class="launch-edit" variant="ghost" size="compact" @click="open({ kind: 'edit', profileId: row.id })">{{ t('launchConfigEditAction') }}</AppButton>
        <IconButton data-launch-menu :label="t('launchConfigActions')" @click="showMenu(row.id, $event)"><span aria-hidden="true">⋯</span></IconButton>
      </div>
      <InlineNotice v-if="!profiles.byCli[cli].length && profiles.status === 'loaded'" :message="t('launchConfigSafeFallback')" />
    </section>
    <Teleport to="body"><AppMenu class="launch-menu" :style="anchor" :open="active && !!menuId" :label="t('launchConfigActions')" :items="menuItems" @close="menuId = null" @select="action" /></Teleport>
    <LaunchConfigurationEditor v-if="editor" :key="editorVersion" :request="editor" :active="active" @close="editor = null" />
  </section>
</template>
<style scoped>
.launch-configurations, .launch-group { display: flex; flex-direction: column; min-width: 0; gap: 12px; }
.launch-configurations { gap: 20px; }
.launch-configurations header, .launch-group h3 { display: flex; align-items: center; gap: 10px; }
.launch-configurations header { justify-content: space-between; flex-wrap: wrap; }
.launch-row { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto 62px 28px; align-items: center; gap: 8px; min-width: 0; padding: 8px; border-bottom: 1px solid var(--border-color); }
.launch-name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.launch-default, .launch-hint { color: var(--text-secondary); font-size: 12px; }
.launch-edit { opacity: 0; }
.launch-row:hover .launch-edit, .launch-row:focus-within .launch-edit { opacity: 1; }
.launch-menu { position: fixed; max-height: calc(100vh - 16px); }
@media (hover: none) { .launch-edit { opacity: 1; } }
</style>
