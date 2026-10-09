<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { TerminalPreferences } from '@/config/terminalPreferences'
const props = defineProps<{ preferences: TerminalPreferences }>()
const { t } = useI18n()
const surface = computed(() => ({ backgroundColor: props.preferences.theme.background, color: props.preferences.theme.foreground,
  fontFamily: props.preferences.fontFamily, fontSize: `${props.preferences.fontSize}px`, lineHeight: String(props.preferences.lineHeight),
  '--preview-cursor': props.preferences.theme.cursor, '--preview-cursor-accent': props.preferences.theme.cursorAccent ?? props.preferences.theme.background }))
</script>
<template>
  <section class="terminal-preview" data-terminal-preview :style="surface" :aria-label="t('terminalPreview')">
    <p class="preview-caption">{{ t('terminalPreviewOnly') }}</p>
    <p><span :style="{ color: preferences.theme.green }">✓</span> {{ t('terminalPreviewReady') }}</p>
    <p><span :style="{ color: preferences.theme.blue }">›</span> Claude Code · Codex CLI</p>
    <p :style="{ color: preferences.theme.yellow }">{{ t('terminalPreviewSample') }}</p>
    <p>❯ <span class="preview-cursor" :class="[`cursor-${preferences.cursorStyle}`, { 'cursor-blink': preferences.cursorBlink }]" aria-hidden="true">&nbsp;</span></p>
  </section>
</template>
<style scoped>
.terminal-preview { min-width: 0; border: 1px solid var(--border-color); border-radius: 8px; padding: 16px; overflow-wrap: anywhere; user-select: text; }
p { margin: 0; white-space: pre-wrap; }
.preview-caption { opacity: 0.75; margin-bottom: 12px; font-size: 11px; }
.preview-cursor { display: inline-block; width: 0.6em; height: 1em; vertical-align: text-bottom; box-sizing: border-box; }
.cursor-bar { border-left: 2px solid var(--preview-cursor); }
.cursor-underline { border-bottom: 2px solid var(--preview-cursor); }
.cursor-block { background: var(--preview-cursor); color: var(--preview-cursor-accent); }
.cursor-blink { animation: preview-blink 1s step-end infinite; }
@keyframes preview-blink { 50% { opacity: 0; } }
@media (prefers-reduced-motion: reduce) { .cursor-blink { animation: none; } }
</style>
