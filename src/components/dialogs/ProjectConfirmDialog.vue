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
watch(() => [props.active, props.request], () => { if (!props.active && props.request) emit('cancel') }, { flush: 'sync', immediate: true })
</script>
<template>
  <AppDialog :open="active && !!request" :title="t(request?.kind === 'remove-project' ? 'projectRemoveTitle' : 'configurationDeleteTitle')"
    :description="t(request?.kind === 'remove-project' ? 'projectRemoveDescription' : 'configurationDeleteDescription')" @close="emit('cancel')">
    <div data-project-confirm><p class="confirm-title">{{ request?.title }}</p><InlineNotice v-if="safeErrorKey" kind="warning" :message="t(safeErrorKey)" /></div>
    <template #footer>
      <AppButton data-cancel-project-action @click="emit('cancel')">{{ t('cancel') }}</AppButton>
      <AppButton v-if="request?.kind === 'remove-project'" data-confirm-project-remove variant="danger" :disabled="busy" :loading="busy" @click="emit('confirm')">{{ t('projectRemoveConfirm') }}</AppButton>
      <AppButton v-else data-confirm-configuration-delete variant="danger" :disabled="busy" :loading="busy" @click="emit('confirm')">{{ t('configurationDeleteConfirm') }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
.confirm-title { overflow-wrap: anywhere; color: var(--text-primary); }
</style>
