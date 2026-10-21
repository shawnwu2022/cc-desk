<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import type { UnifiedCliKind } from '@/types/unifiedSession'
import claude from '@/assets/icons/cli/claude.svg'
import codex from '@/assets/icons/cli/codex.svg'
import opencode from '@/assets/icons/cli/opencode.svg'
import opencodeDark from '@/assets/icons/cli/opencode-dark.svg'

const props = defineProps<{ cli: UnifiedCliKind; decorative?: boolean }>()
const apps: Record<UnifiedCliKind, { src: string; darkSrc?: string; name: string; fallback: string }> = {
  claude: { src: claude, name: 'Claude Code', fallback: 'CC' },
  codex: { src: codex, name: 'Codex CLI', fallback: 'CX' },
  opencode: { src: opencode, darkSrc: opencodeDark, name: 'opencode', fallback: 'OC' },
}
const app = computed(() => apps[props.cli])
const image = ref<HTMLImageElement>()
const darkImage = ref<HTMLImageElement>()
const imageFailed = ref(false)
watch(() => props.cli, () => { imageFailed.value = false }, { flush: 'sync' })
function onImageError(event: Event) {
  // A detached image's late event cannot replace the current CLI mark.
  if (event.currentTarget === image.value || event.currentTarget === darkImage.value) imageFailed.value = true
}
</script>

<template>
  <component :is="decorative ? 'span' : AppTooltip" :text="decorative ? undefined : app.name">
    <span class="cli-app-icon" :class="`cli-app-icon--${cli}`" :role="decorative ? undefined : 'img'" :aria-hidden="decorative || undefined" :aria-label="decorative ? undefined : app.name" :tabindex="decorative ? undefined : 0">
      <template v-if="!imageFailed">
        <img :key="cli" ref="image" class="cli-app-icon__image" :class="{ 'cli-app-icon__image--light': app.darkSrc }" :src="app.src" alt="" aria-hidden="true" draggable="false" @error="onImageError" />
        <img v-if="app.darkSrc" :key="`${cli}-dark`" ref="darkImage" class="cli-app-icon__image cli-app-icon__image--dark" :src="app.darkSrc" alt="" aria-hidden="true" draggable="false" @error="onImageError" />
      </template>
      <span v-else class="cli-app-icon__fallback" aria-hidden="true">{{ app.fallback }}</span>
    </span>
  </component>
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
/* OpenCode 使用官方成对配色，随 GUI 主题切换，不套用全局图片反色。 */
.cli-app-icon .cli-app-icon__image--dark { display: none; }
[data-theme="dark"] .cli-app-icon .cli-app-icon__image--light { display: none; }
[data-theme="dark"] .cli-app-icon .cli-app-icon__image--dark { display: block; }
.cli-app-icon__fallback { font-family: var(--font-sans); font-size: 9px; font-weight: 600; line-height: 16px; }
</style>
