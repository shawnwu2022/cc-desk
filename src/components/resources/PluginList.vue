<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
import ResourceMetadata from './ResourceMetadata.vue'
defineProps<{ items: Extract<ProjectResourceItem, { type: 'plugin' }>[] }>()
const { t } = useI18n()
</script>
<template>
  <ul class="resource-list">
    <li v-for="(item, index) in items" :key="index" class="resource-card">
      <h3>{{ item.name ?? t('resourceHidden') }}</h3>
      <p v-if="item.version" class="resource-detail">{{ t('resourceVersion') }}: {{ item.version }}</p>
      <p class="resource-detail">{{ t('resourceEnabled') }}: {{ t(item.enabled === null ? 'resourceUnknown' : item.enabled ? 'resourceYesEnabled' : 'resourceNoEnabled') }}</p>
      <p class="resource-detail">{{ t('resourceInstalled') }}: {{ t(item.installed === null ? 'resourceUnknown' : item.installed ? 'resourceYesInstalled' : 'resourceNoInstalled') }}</p>
      <ResourceMetadata :item="item" />
    </li>
  </ul>
</template>
