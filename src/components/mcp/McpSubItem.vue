<template>
  <div class="mcp-sub-item" :class="{ expanded: isExpanded }">
    <!-- Item Header -->
    <div class="sub-header" @click="toggleExpand">
      <img
        class="expand-icon"
        :class="{ expanded: isExpanded }"
        src="@/assets/icons/chevron.svg"
        alt="Toggle"
      />
      <span class="sub-name">{{ name }}</span>
    </div>

    <!-- Expanded Details -->
    <div v-if="isExpanded" class="sub-details">
      <!-- Description -->
      <div v-if="description" class="sub-description">
        {{ description }}
      </div>
      <div v-else class="sub-description-empty">
        {{ t('noDescription') }}
      </div>

      <!-- Parameters (for tools, parsed from inputSchema) -->
      <div v-if="parsedParams.length > 0" class="sub-arguments">
        <div class="args-title">{{ t('parameters') }}</div>
        <div class="args-list">
          <div v-for="param in parsedParams" :key="param.name" class="arg-item">
            <div class="arg-header">
              <span class="arg-name">{{ param.name }}</span>
              <span v-if="param.type" class="arg-type">{{ param.type }}</span>
              <span v-if="param.required" class="arg-required">{{ t('required') }}</span>
            </div>
            <div v-if="param.description" class="arg-desc">{{ param.description }}</div>
          </div>
        </div>
      </div>

      <!-- Arguments (for prompts) -->
      <div v-else-if="arguments && arguments.length > 0" class="sub-arguments">
        <div class="args-title">{{ t('parameters') }}</div>
        <div class="args-list">
          <div v-for="arg in arguments" :key="arg.name" class="arg-item">
            <div class="arg-header">
              <span class="arg-name">{{ arg.name }}</span>
              <span v-if="arg.required" class="arg-required">{{ t('required') }}</span>
            </div>
            <div v-if="arg.description" class="arg-desc">{{ arg.description }}</div>
          </div>
        </div>
      </div>

      <!-- URI (for resources) -->
      <div v-if="uri" class="sub-uri">
        <div class="uri-title">{{ t('uri') }}</div>
        <div class="uri-value">{{ uri }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

interface PromptArgument {
  name: string
  description?: string
  required: boolean
}

interface ParsedParam {
  name: string
  type?: string
  description?: string
  required: boolean
}

const props = defineProps<{
  name: string
  description?: string
  type: 'tool' | 'prompt' | 'resource'
  arguments?: PromptArgument[]
  uri?: string
  inputSchema?: Record<string, unknown>
}>()

const isExpanded = ref(false)

const parsedParams = computed<ParsedParam[]>(() => {
  if (props.type !== 'tool' || !props.inputSchema) return []
  const schema = props.inputSchema
  const properties = schema.properties as Record<string, { type?: string; description?: string }> | undefined
  const required = (schema.required as string[]) || []
  if (!properties) return []
  return Object.entries(properties).map(([name, prop]) => ({
    name,
    type: prop.type,
    description: prop.description,
    required: required.includes(name),
  }))
})

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}
</script>

<style scoped>
.mcp-sub-item {
  background: var(--bg-tertiary);
  border-radius: 4px;
  padding: 4px 8px;
  transition: background 0.15s ease;
}

.mcp-sub-item:hover {
  background: var(--hover-bg);
}

.mcp-sub-item.expanded {
  background: var(--bg-secondary);
}

.sub-header {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.expand-icon {
  width: 12px;
  height: 12px;
  color: var(--text-secondary);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.expand-icon.expanded {
  transform: rotate(90deg);
}

.sub-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sub-details {
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sub-description {
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.4;
}

.sub-description-empty {
  font-size: 11px;
  color: var(--text-tertiary);
  font-style: italic;
}

.sub-arguments {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.arg-type {
  font-size: 9px;
  padding: 1px 4px;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border-radius: 3px;
  font-family: var(--font-mono);
}

.args-title,
.uri-title {
  font-size: 10px;
  font-weight: 500;
  color: var(--text-tertiary);
}

.args-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.arg-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 11px;
}

.arg-header {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.arg-name {
  color: var(--text-primary);
  font-family: var(--font-mono);
}

.arg-required {
  font-size: 9px;
  padding: 1px 4px;
  background: var(--accent-bg);
  color: var(--accent-color);
  border-radius: 3px;
}

.arg-desc {
  color: var(--text-tertiary);
  line-height: 1.4;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: break-word;
}

.sub-uri {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.uri-value {
  font-size: 11px;
  color: var(--text-primary);
  font-family: var(--font-mono);
  word-break: break-all;
}
</style>