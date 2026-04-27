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
            <section class="shortcuts-section">
              <h3>Claude Code Shortcuts</h3>
              <p class="hint">These shortcuts are passed directly to the terminal.</p>
              <div class="shortcuts-list">
                <div class="shortcut-item">
                  <kbd>Ctrl+C</kbd>
                  <span>Cancel current input / interrupt</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+D</kbd>
                  <span>Exit Claude session</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Alt+P</kbd>
                  <span>Enter plan mode</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+L</kbd>
                  <span>Clear terminal screen</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+R</kbd>
                  <span>Search command history</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+W</kbd>
                  <span>Delete word backward</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+K</kbd>
                  <span>Clear line after cursor</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+U</kbd>
                  <span>Clear line before cursor</span>
                </div>
              </div>
            </section>

            <section class="shortcuts-section">
              <h3>Application Shortcuts</h3>
              <div class="shortcuts-list">
                <div class="shortcut-item">
                  <kbd>Ctrl+Shift+N</kbd>
                  <span>New window</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+Shift+R</kbd>
                  <span>Restart application</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+,</kbd>
                  <span>Settings</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+Plus/-</kbd>
                  <span>Zoom in/out</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+0</kbd>
                  <span>Reset zoom</span>
                </div>
                <div class="shortcut-item">
                  <kbd>Ctrl+Full Screen</kbd>
                  <span>Toggle fullscreen</span>
                </div>
                <div class="shortcut-item">
                  <kbd>F11</kbd>
                  <span>Toggle fullscreen</span>
                </div>
              </div>
            </section>

            <section class="shortcuts-section">
              <h3>Slash Commands</h3>
              <p class="hint">Type these in Claude prompt:</p>
              <div class="shortcuts-list">
                <div class="shortcut-item">
                  <kbd>/help</kbd>
                  <span>Show available commands</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/clear</kbd>
                  <span>Clear conversation</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/compact</kbd>
                  <span>Compact conversation history</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/cost</kbd>
                  <span>Show session cost</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/model</kbd>
                  <span>Switch model</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/permissions</kbd>
                  <span>Review permissions</span>
                </div>
                <div class="shortcut-item">
                  <kbd>/init</kbd>
                  <span>Initialize CLAUDE.md</span>
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
defineProps<{
  visible: boolean
}>()

defineEmits<{
  close: []
}>()
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
  width: 420px;
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
  gap: 6px;
}

.shortcut-item {
  display: flex;
  align-items: center;
  gap: 12px;
}

kbd {
  display: inline-block;
  padding: 4px 8px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-family: 'SF Mono', 'Consolas', 'Monaco', 'Menlo', monospace;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  min-width: 80px;
  text-align: center;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
}

.shortcut-item span {
  font-size: 13px;
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