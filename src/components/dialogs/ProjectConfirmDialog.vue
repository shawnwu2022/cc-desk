<script setup lang="ts">
import en from '@/i18n/locales/en'
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ProjectConfirmationRequest } from '@/types/confirmation'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const props = withDefaults(defineProps<{ request: ProjectConfirmationRequest | null; active?: boolean; busy?: boolean; errorKey?: string | null }>(), { active: true, busy: false, errorKey: null })
const emit = defineEmits<{ confirm: []; cancel: [] }>()
const { t } = useI18n()
const safeErrorKey = computed(() => props.errorKey && Object.prototype.hasOwnProperty.call(en, props.errorKey) ? props.errorKey : props.errorKey ? 'errorGenericUnavailable' : null)
const keys = computed(() => props.request?.kind === 'close-project-sessions'
  ? { title: 'projectCloseSessionsTitle', description: 'projectCloseSessionsDescription', action: 'projectCloseSessions' }
  : props.request?.kind === 'remove-project'
    ? { title: 'projectRemoveTitle', description: 'projectRemoveDescription', action: 'projectRemoveConfirm' }
    : { title: 'configurationDeleteTitle', description: 'configurationDeleteDescription', action: 'configurationDeleteConfirm' })
watch(() => [props.active, props.request], () => { if (!props.active && props.request) emit('cancel') }, { flush: 'sync', immediate: true })
</script>
<template>
  <AppDialog :open="active && !!request" :title="t(keys.title)"
    :description="t(keys.description)" @close="emit('cancel')">
    <div data-project-confirm><p class="confirm-title">{{ request?.title }}</p><InlineNotice v-if="safeErrorKey" kind="warning" :message="t(safeErrorKey)" /></div>
    <template #footer>
      <AppButton data-cancel-project-action @click="emit('cancel')">{{ t('cancel') }}</AppButton>
      <AppButton :data-confirm-project-remove="request?.kind === 'remove-project' ? '' : undefined"
        :data-confirm-project-close="request?.kind === 'close-project-sessions' ? '' : undefined"
        :data-confirm-configuration-delete="request?.kind === 'delete-launch-configuration' ? '' : undefined"
        variant="danger" :disabled="busy" :loading="busy" @click="emit('confirm')">{{ t(keys.action) }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
.confirm-title { overflow-wrap: anywhere; color: var(--text-primary); }
</style>
