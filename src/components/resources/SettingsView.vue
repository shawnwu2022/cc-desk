<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
import ResourceMetadata from './ResourceMetadata.vue'
defineProps<{ items: Extract<ProjectResourceItem, { type: 'setting' }>[] }>()
const { t } = useI18n()
</script>
<template>
  <dl class="resource-list">
    <div v-for="(item, index) in items" :key="index" class="resource-card">
      <dt>{{ t(`resourceSetting_${item.name}`) }}</dt>
      <dd>{{ item.value ?? t('resourceHidden') }}</dd>
      <ResourceMetadata :item="item" />
    </div>
  </dl>
</template>
