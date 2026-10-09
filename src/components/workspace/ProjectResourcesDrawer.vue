<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useProjectResourcesStore } from '@/stores/projectResources'
import type { ProjectResourceKind } from '@/types/projectResources'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import LoadingState from '@/components/ui/LoadingState.vue'
import InstructionsView from '@/components/resources/InstructionsView.vue'
import ResourceSettingsView from '@/components/resources/SettingsView.vue'
import McpList from '@/components/resources/McpList.vue'
import SkillList from '@/components/resources/SkillList.vue'
import AgentList from '@/components/resources/AgentList.vue'
import PluginList from '@/components/resources/PluginList.vue'
import '@/components/resources/resources.css'
const { t } = useI18n()
const resources = useProjectResourcesStore()
const kinds: ProjectResourceKind[] = ['instructions', 'config', 'mcp', 'skills', 'agents', 'plugins']
const options = computed(() => kinds.map(value => ({ value, label: t(`resourceCategory_${value}`) })))
function select(value: string) { if (kinds.includes(value as ProjectResourceKind)) resources.kind = value as ProjectResourceKind }
const documents = computed(() => resources.items.filter(item => item.type === 'document'))
const settings = computed(() => resources.items.filter(item => item.type === 'setting'))
const mcps = computed(() => resources.items.filter(item => item.type === 'mcp'))
const skills = computed(() => resources.items.filter(item => item.type === 'skill'))
const agents = computed(() => resources.items.filter(item => item.type === 'agent'))
const plugins = computed(() => resources.items.filter(item => item.type === 'plugin'))
</script>

<template>
  <section class="project-resources" :aria-label="t('contextResources')" :aria-busy="resources.loading" data-project-resources>
    <p class="resource-hint">{{ t('resourcesReadOnly') }}</p>
    <div class="resource-controls">
      <AppSelect :model-value="resources.kind" :options="options" :label="t('resourceCategory')" size="compact" @update:model-value="select" />
      <AppButton size="compact" :disabled="!resources.context || resources.loading" @click="resources.refresh">{{ t('refresh') }}</AppButton>
    </div>
    <EmptyState v-if="!resources.hasSession" :title="t('resourcesChooseSession')" :description="t('resourcesChooseSessionHint')" />
    <template v-else>
      <InlineNotice v-if="resources.legacyProjectOnly" :message="t('resourcesProjectOnly')" />
      <InlineNotice v-if="resources.unavailable" kind="warning" :message="t(resources.error === 'SOURCE_UNSUPPORTED' ? 'resourcesUnsupported' : 'resourcesUnavailable')" />
      <InlineNotice v-if="resources.stale" :message="t(resources.loading ? 'resourcesRefreshing' : 'resourcesStale')" />
      <LoadingState v-if="resources.loading" :label="t('resourcesLoading')" :rows="1" />
      <InlineNotice v-if="resources.partial && !resources.legacyProjectOnly" :message="t('resourcesPartial')" />
      <EmptyState v-if="!resources.loading && !resources.unavailable && (!resources.partial || resources.legacyProjectOnly) && !resources.items.length" :title="t('resourcesEmpty')" :description="t(resources.legacyProjectOnly ? 'resourcesProjectEmptyHint' : 'resourcesEmptyHint')" />
      <InstructionsView v-if="resources.kind === 'instructions'" :items="documents" />
      <ResourceSettingsView v-else-if="resources.kind === 'config'" :items="settings" />
      <McpList v-else-if="resources.kind === 'mcp'" :items="mcps" />
      <SkillList v-else-if="resources.kind === 'skills'" :items="skills" />
      <AgentList v-else-if="resources.kind === 'agents'" :items="agents" />
      <PluginList v-else-if="resources.kind === 'plugins'" :items="plugins" />
    </template>
  </section>
</template>
