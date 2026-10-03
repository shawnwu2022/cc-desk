<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
import ResourceMetadata from './ResourceMetadata.vue'
defineProps<{ items: Extract<ProjectResourceItem, { type: 'agent' }>[] }>()
const { t } = useI18n()
</script>
<template>
  <ul class="resource-list">
    <li v-for="(item, index) in items" :key="index" class="resource-card">
      <h3>{{ item.name ?? t('resourceHidden') }}</h3>
      <p v-if="item.description" class="resource-description">{{ item.description }}</p>
      <p v-if="item.model" class="resource-detail">{{ t('resourceSetting_model') }}: {{ item.model }}</p>
      <ResourceMetadata :item="item" />
    </li>
  </ul>
</template>
