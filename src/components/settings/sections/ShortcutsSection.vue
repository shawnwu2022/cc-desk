<template>
  <div class="section-content">
    <h2 class="section-heading">Keyboard Shortcuts</h2>

    <div class="shortcuts-group" v-for="group in filteredGroups" :key="group.title">
      <h3 class="group-title">{{ group.title }}</h3>
      <p v-if="group.hint" class="group-hint">{{ group.hint }}</p>
      <div class="shortcuts-table">
        <div class="shortcut-row" v-for="item in group.items" :key="item.key">
          <kbd>{{ item.key }}</kbd>
          <span class="shortcut-desc">{{ item.desc }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const isMac = computed(() => navigator.userAgent.includes('Mac'))
const alt = computed(() => isMac.value ? 'Option' : 'Alt')

const filteredGroups = computed(() => [
  {
    title: 'Application Shortcuts',
    hint: '',
    items: [
      { key: 'Ctrl+Shift+N', desc: 'Open new window' },
      { key: 'Ctrl+Shift+← / →', desc: 'Snap window to left / right half' },
      { key: 'Ctrl+Shift+R', desc: 'Restart application' },
      { key: 'Ctrl+Shift+H', desc: 'Back to project select' },
      { key: 'Ctrl+,', desc: 'Toggle settings' },
      { key: 'Ctrl+Plus / −', desc: 'Increase / decrease font size' },
      { key: 'Ctrl+0', desc: 'Reset font size' },
    ]
  },
  {
    title: 'Session Management',
    hint: 'Only available in terminal view.',
    items: [
      { key: `${alt.value}+N`, desc: 'New session' },
      { key: `${alt.value}+R`, desc: 'Restart session' },
      { key: `${alt.value}+↑ / ↓`, desc: 'Switch to previous / next tab' },
    ]
  },
  {
    title: 'Claude Code Shortcuts',
    hint: 'Shortcuts passed directly to the Claude terminal.',
    items: [
      { key: 'Ctrl+C', desc: 'Cancel current input or generation' },
      { key: 'Ctrl+D', desc: 'Exit Claude Code session' },
      { key: `${alt.value}+P`, desc: 'Switch model without clearing prompt' },
      { key: `${alt.value}+T`, desc: 'Toggle extended thinking' },
      { key: `${alt.value}+O`, desc: 'Toggle fast mode' },
      { key: 'Ctrl+L', desc: 'Clear prompt input and redraw screen' },
      { key: 'Ctrl+R', desc: 'Reverse search command history' },
      { key: 'Ctrl+O', desc: 'Toggle transcript viewer' },
      { key: 'Ctrl+B', desc: 'Run task in background' },
      { key: 'Ctrl+T', desc: 'Toggle task list' },
      { key: 'Esc Esc', desc: 'Rewind or summarize' },
    ]
  },
  {
    title: 'Text Editing',
    hint: '',
    items: [
      { key: 'Ctrl+A', desc: 'Move cursor to start of line' },
      { key: 'Ctrl+E', desc: 'Move cursor to end of line' },
      { key: 'Ctrl+W', desc: 'Delete previous word' },
      { key: 'Ctrl+K', desc: 'Delete to end of line' },
      { key: 'Ctrl+U', desc: 'Delete from cursor to start of line' },
      { key: 'Ctrl+Y', desc: 'Paste deleted text' },
      { key: `${alt.value}+B`, desc: 'Move cursor back one word' },
      { key: `${alt.value}+F`, desc: 'Move cursor forward one word' },
    ]
  },
  {
    title: 'Multi-line Input',
    hint: '',
    items: [
      { key: '\\ + Enter', desc: 'Insert newline' },
      { key: 'Ctrl+J', desc: 'Insert newline (any terminal)' },
      ...(isMac.value
        ? [{ key: 'Shift+Enter', desc: 'Insert newline (iTerm2, WezTerm, etc.)' }]
        : [{ key: 'Shift+Enter', desc: 'Insert newline (if terminal supports it)' }]
      ),
    ]
  },
  {
    title: 'Quick Input',
    hint: '',
    items: [
      { key: '/ at start', desc: 'Command or skill' },
      { key: '! at start', desc: 'Bash mode — run command directly' },
      { key: '@', desc: 'File path mention with autocomplete' },
    ]
  },
  {
    title: 'Slash Commands',
    hint: 'Type in the Claude prompt:',
    items: [
      { key: '/clear', desc: 'Start new conversation (alias /reset, /new)' },
      { key: '/compact', desc: 'Summarize to free context window' },
      { key: '/model', desc: 'Switch AI model' },
      { key: '/cost', desc: 'Show session cost (alias /usage, /stats)' },
      { key: '/permissions', desc: 'Manage tool permission rules' },
      { key: '/init', desc: 'Initialize CLAUDE.md for project' },
      { key: '/config', desc: 'Open settings (alias /settings)' },
      { key: '/resume', desc: 'Resume a previous session (alias /continue)' },
      { key: '/diff', desc: 'Interactive diff viewer for changes' },
      { key: '/help', desc: 'Show available commands' },
      { key: '/context', desc: 'Visualize context window usage' },
      { key: '/doctor', desc: 'Diagnose Claude Code installation' },
      { key: '/theme', desc: 'Change color theme' },
      { key: '/memory', desc: 'Edit CLAUDE.md memory files' },
      { key: '/rename', desc: 'Rename current session' },
      { key: '/btw <q>', desc: 'Quick side question without context' },
      { key: '/plan', desc: 'Enter plan mode' },
      { key: '/branch', desc: 'Branch conversation (alias /fork)' },
      { key: '/copy', desc: 'Copy last response to clipboard' },
      { key: '/review', desc: 'Review pull request in session' },
      { key: '/exit', desc: 'Exit CLI (alias /quit)' },
    ]
  },
])
</script>

<style scoped>
.section-content {
  padding: 8px 0;
}

.section-heading {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 24px;
}

.shortcuts-group {
  margin-bottom: 24px;
}

.group-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 4px;
}

.group-hint {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 12px;
}

.shortcuts-table {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.shortcut-row {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 6px 8px;
  border-radius: 4px;
}

.shortcut-row:hover {
  background: var(--bg-secondary);
}

kbd {
  display: inline-block;
  padding: 4px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  min-width: 120px;
  text-align: center;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
}

.shortcut-desc {
  font-size: 13px;
  color: var(--text-primary);
}
</style>
