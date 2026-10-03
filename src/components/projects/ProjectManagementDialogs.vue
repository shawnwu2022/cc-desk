<script setup lang="ts">
import ProjectConfirmDialog from '@/components/dialogs/ProjectConfirmDialog.vue'
import { projectBasename } from '@/utils/displayName'
import { useI18n } from 'vue-i18n'
import { useProjectManagementStore } from '@/stores/projectManagement'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const { t } = useI18n()
const management = useProjectManagementStore()
</script>
<template>
  <ProjectConfirmDialog :request="management.dialog?.kind === 'remove' ? { kind: 'remove-project', title: management.groups.find(group => group.projectKey === management.dialog!.project.projectKey)?.name ?? projectBasename(management.dialog.project.projectPath) } : null" :busy="management.busy" :error-key="management.error" @cancel="management.closeDialog" @confirm="management.remove" />
  <AppDialog :open="management.dialog?.kind === 'rename'" :title="t('rename')" @close="management.closeDialog">
    <p class="project-target" :title="management.dialog?.project.projectPath">{{ management.dialog?.project.projectPath }}</p>
    <AppInput v-if="management.dialog?.kind === 'rename'" v-model="management.renameValue" :label="t('projectDisplayName')" :disabled="management.busy" @keydown.enter="management.rename" />
    <InlineNotice v-if="management.renameError" kind="warning" :message="t(management.renameError)" />
    <InlineNotice v-if="management.error" kind="warning" :message="t(management.error)" />
    <template #footer>
      <AppButton data-cancel-project-action :disabled="management.busy" @click="management.closeDialog">{{ t('cancel') }}</AppButton>
      <AppButton data-confirm-project-rename variant="primary" :loading="management.busy" :disabled="management.busy" @click="management.rename">{{ t('save') }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
.project-target { min-width: 0; overflow-wrap: anywhere; color: var(--text-secondary); font-size: 12px; }
</style>
