<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="visible" class="modal-overlay" @click.self="$emit('close')">
        <div class="modal-content">
          <header class="modal-header">
            <h2>Keyboard Shortcuts</h2>
            <button class="close-btn" @click="$emit('close')">
              <img src="@/assets/icons/close.svg" alt="Close" />
            </button>
          </header>

          <div class="modal-body">
            <section v-for="group in groups" :key="group.title" class="shortcuts-section">
              <h3>{{ group.title }}</h3>
              <p v-if="group.hint" class="hint">{{ group.hint }}</p>
              <div class="shortcuts-list">
                <div v-for="item in group.items" :key="item.key" class="shortcut-item">
                  <kbd>{{ item.key }}</kbd>
                  <span>{{ item.desc }}</span>
                </div>
              </div>
            </section>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ctrl, alt, cmd, isMac } from '@/utils/platform'

defineProps<{
  visible: boolean
}>()

defineEmits<{
  close: []
}>()

const groups = [
  {
    title: 'Application Shortcuts',
    items: [
      { key: `${cmd}+Shift+N`, desc: 'New window' },
      { key: `${cmd}+Shift+← / →`, desc: 'Snap window to left / right half' },
      { key: `${cmd}+Shift+R`, desc: 'Restart application' },
      { key: `${cmd}+Shift+H`, desc: 'Toggle home / terminal' },
      { key: `${cmd}+Shift+/`, desc: 'Show keyboard shortcuts' },
      { key: `${cmd}+Shift+S`, desc: 'Toggle sessions panel' },
      { key: `${cmd},`, desc: 'Settings' },
      { key: `${cmd}+Plus / −`, desc: 'Zoom in / out' },
      { key: `${cmd}+0`, desc: 'Reset zoom' },
    ]
  },
  {
    title: 'Session Management',
    items: [
      { key: `${alt}+N`, desc: 'New session' },
      { key: `${alt}+R`, desc: 'Restart session' },
      { key: `${alt}+W`, desc: 'Close current tab' },
      { key: `${ctrl}+Tab`, desc: 'Switch to next tab' },
      { key: `${ctrl}+Shift+Tab`, desc: 'Switch to previous tab' },
      { key: `${alt}+↑ / ↓`, desc: 'Switch to previous / next tab' },
    ]
  },
  {
    title: 'Claude Code Shortcuts',
    hint: 'Passed directly to the terminal.',
    items: [
      { key: `${ctrl}+C`, desc: 'Cancel current input or generation' },
      { key: `${ctrl}+D`, desc: 'Exit Claude session' },
      { key: `${alt}+P`, desc: 'Switch model without clearing prompt' },
      { key: `${alt}+T`, desc: 'Toggle extended thinking' },
      { key: `${alt}+O`, desc: 'Toggle fast mode' },
      { key: `${ctrl}+L`, desc: 'Clear screen' },
      { key: `${ctrl}+R`, desc: 'Search command history' },
      { key: `${ctrl}+O`, desc: 'Toggle transcript viewer' },
      { key: `${ctrl}+B`, desc: 'Run task in background' },
      { key: `${ctrl}+T`, desc: 'Toggle task list' },
      { key: 'Esc Esc', desc: 'Rewind or summarize' },
    ]
  },
  {
    title: 'Text Editing',
    items: [
      { key: `${ctrl}+A`, desc: 'Move cursor to start of line' },
      { key: `${ctrl}+E`, desc: 'Move cursor to end of line' },
      { key: `${ctrl}+W`, desc: 'Delete word backward' },
      { key: `${ctrl}+K`, desc: 'Delete to end of line' },
      { key: `${ctrl}+U`, desc: 'Delete from cursor to start of line' },
      { key: `${ctrl}+Y`, desc: 'Paste deleted text' },
      { key: `${alt}+B`, desc: 'Move cursor back one word' },
      { key: `${alt}+F`, desc: 'Move cursor forward one word' },
    ]
  },
  {
    title: 'Multi-line Input',
    items: [
      { key: '\\ + Enter', desc: 'Insert newline' },
      { key: `${ctrl}+J`, desc: 'Insert newline (any terminal)' },
      ...(isMac
        ? [{ key: 'Shift+Enter', desc: 'Insert newline (iTerm2, WezTerm, etc.)' }]
        : [{ key: 'Shift+Enter', desc: 'Insert newline (if terminal supports it)' }]
      ),
    ]
  },
  {
    title: 'Quick Input',
    items: [
      { key: '/ at start', desc: 'Command or skill' },
      { key: '! at start', desc: 'Bash mode' },
      { key: '@', desc: 'File path mention' },
    ]
  },
  {
    title: 'Slash Commands',
    hint: 'Type in Claude prompt:',
    items: [
      { key: '/help', desc: 'Show available commands' },
      { key: '/clear', desc: 'Start new conversation' },
      { key: '/compact', desc: 'Summarize to free context window' },
      { key: '/model', desc: 'Switch AI model' },
      { key: '/cost', desc: 'Show session cost' },
      { key: '/permissions', desc: 'Manage tool permission rules' },
      { key: '/config', desc: 'Open settings' },
      { key: '/init', desc: 'Initialize CLAUDE.md' },
      { key: '/resume', desc: 'Resume a previous session' },
      { key: '/diff', desc: 'Interactive diff viewer' },
      { key: '/plan', desc: 'Enter plan mode' },
      { key: '/review', desc: 'Review pull request' },
      { key: '/doctor', desc: 'Diagnose installation' },
      { key: '/exit', desc: 'Exit CLI' },
    ]
  },
]
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.modal-content {
  width: 460px;
  max-height: 80vh;
  background: var(--bg-primary);
  border-radius: 12px;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.12);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.modal-header h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.close-btn img {
  width: 16px;
  height: 16px;
}

.close-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.modal-body {
  padding: 20px;
  max-height: calc(80vh - 93px);
  overflow-y: auto;
}

.shortcuts-section {
  margin-bottom: 20px;
}

.shortcuts-section:last-child {
  margin-bottom: 0;
}

.shortcuts-section h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.hint {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 10px;
}

.shortcuts-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.shortcut-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 6px;
  border-radius: 4px;
}

.shortcut-item:hover {
  background: var(--bg-secondary);
}

kbd {
  display: inline-block;
  padding: 3px 8px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-family: 'SF Mono', 'Consolas', 'Monaco', 'Menlo', monospace;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-primary);
  min-width: 100px;
  text-align: center;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
}

.shortcut-item span {
  font-size: 12px;
  color: var(--text-primary);
}

/* Transitions */
.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.2s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}
</style>
