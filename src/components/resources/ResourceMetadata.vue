<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
defineProps<{ item: ProjectResourceItem }>()
const { t } = useI18n()
</script>
<template>
  <p class="resource-origin">{{ t(`resourceOrigin_${item.origin}`) }}</p>
  <p v-if="item.withheld" class="resource-privacy">{{ t('resourceHidden') }}</p>
</template>
