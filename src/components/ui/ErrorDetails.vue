<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { mapSafeUserError, type UserErrorContext } from '@/utils/userError'
const props = withDefaults(defineProps<{ code: string; context?: UserErrorContext }>(), { context: 'workspace' })
const { t } = useI18n()
const detailCode = computed(() => mapSafeUserError(props.code, props.context).detailCode || 'GENERIC_UNAVAILABLE')
</script>

<template>
  <details class="ui-error-details">
    <summary>{{ t('toggleDetails') }}</summary>
    <code>{{ detailCode }}</code>
  </details>
</template>
