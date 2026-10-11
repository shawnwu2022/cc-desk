<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
import ResourceMetadata from './ResourceMetadata.vue'
defineProps<{ items: Extract<ProjectResourceItem, { type: 'mcp' }>[] }>()
const { t } = useI18n()
</script>
<template>
  <ul class="resource-list">
    <li v-for="(item, index) in items" :key="index" class="resource-card">
      <h3>{{ item.name ?? t('resourceHidden') }}</h3>
      <p class="resource-detail">{{ t('resourceTransport') }}: {{ t(`resourceTransport_${item.transport}`) }}</p>
      <ResourceMetadata :item="item" />
    </li>
  </ul>
</template>
