<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import type { UnifiedCliKind } from '@/types/unifiedSession'
import claude from '@/assets/icons/cli/claude.svg'
import codex from '@/assets/icons/cli/codex.svg'

const props = defineProps<{ cli: UnifiedCliKind }>()
const apps: Record<UnifiedCliKind, { src: string; name: string; fallback: string }> = {
  claude: { src: claude, name: 'Claude Code', fallback: 'CC' },
  codex: { src: codex, name: 'Codex CLI', fallback: 'CX' },
}
const app = computed(() => apps[props.cli])
const image = ref<HTMLImageElement>()
const imageFailed = ref(false)
watch(() => props.cli, () => { imageFailed.value = false }, { flush: 'sync' })
function onImageError(event: Event) {
  // A detached image's late event cannot replace the current CLI mark.
  if (event.currentTarget === image.value) imageFailed.value = true
}
</script>

<template>
  <AppTooltip :text="app.name">
    <span class="cli-app-icon" :class="`cli-app-icon--${cli}`" role="img" :aria-label="app.name" tabindex="0">
      <img v-if="!imageFailed" :key="cli" ref="image" class="cli-app-icon__image" :src="app.src" alt="" aria-hidden="true" draggable="false" @error="onImageError" />
      <span v-else class="cli-app-icon__fallback" aria-hidden="true">{{ app.fallback }}</span>
    </span>
  </AppTooltip>
</template>

<style scoped>
.cli-app-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  vertical-align: middle;
}
.cli-app-icon:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
/* Preserve Claude's brand color; Codex's ChatGPT/OpenAI knot follows the GUI theme. */
.cli-app-icon .cli-app-icon__image { display: block; width: 16px; height: 16px; filter: none; }
[data-theme="dark"] .cli-app-icon--codex .cli-app-icon__image { filter: invert(1); }
.cli-app-icon__fallback { font-family: var(--font-sans); font-size: 9px; font-weight: 600; line-height: 16px; }
</style>
