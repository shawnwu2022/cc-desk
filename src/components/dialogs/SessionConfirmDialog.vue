<script setup lang="ts">
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n()
const sessions = useUnifiedSessionsStore()
const keys = computed(() => sessions.sessionConfirmation?.kind === 'close-running'
  ? { title: 'confirmCloseTitle', description: 'confirmCloseDescription', action: 'confirmCloseAction' }
  : sessions.sessionConfirmation?.kind === 'stop-and-archive'
    ? { title: 'confirmArchiveTitle', description: 'confirmArchiveDescription', action: 'confirmArchiveAction' }
    : { title: 'confirmRestartTitle', description: 'confirmRestartDescription', action: 'confirmRestartAction' })
watch(() => [props.active, sessions.sessionConfirmation], () => { if (!props.active) sessions.closeSessionConfirmation() }, { flush: 'sync', immediate: true })
</script>
<template>
  <AppDialog :open="active && !!sessions.sessionConfirmation" :title="t(keys.title)" :description="t(keys.description)" @close="sessions.closeSessionConfirmation">
    <p class="confirmation-target">{{ sessions.sessionConfirmation?.title }}</p>
    <InlineNotice v-if="sessions.confirmationError" :kind="sessions.confirmationError.severity" :message="t(sessions.confirmationError.messageKey)" />
    <ErrorDetails v-if="sessions.confirmationError" :code="sessions.confirmationError.detailCode" context="session" />
    <template #footer>
      <AppButton @click="sessions.closeSessionConfirmation">{{ t('cancel') }}</AppButton>
      <AppButton data-session-confirm variant="danger" :disabled="sessions.confirmationBusy" :loading="sessions.confirmationBusy" @click="sessions.confirmSessionAction">{{ t(keys.action) }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
.confirmation-target { overflow-wrap: anywhere; }
</style>
