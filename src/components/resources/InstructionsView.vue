<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { ProjectResourceItem } from '@/types/projectResources'
import ResourceMetadata from './ResourceMetadata.vue'
defineProps<{ items: Extract<ProjectResourceItem, { type: 'document' }>[] }>()
const { t } = useI18n()
</script>
<template>
  <div class="resource-list">
    <article v-for="(item, index) in items" :key="index" class="resource-card">
      <h3>{{ item.name ?? t('resourceHidden') }}</h3>
      <ResourceMetadata :item="item" />
      <p v-if="item.text" class="resource-document">{{ item.text }}</p>
      <p v-if="item.truncated" class="resource-detail">{{ t('resourceTruncated') }}</p>
    </article>
  </div>
</template>
