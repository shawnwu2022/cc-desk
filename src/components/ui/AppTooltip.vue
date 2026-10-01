<script lang="ts">
import { cloneVNode, computed, defineComponent, h, ref, watch, nextTick, onBeforeUnmount } from 'vue'

let nextId = 0

/** The default slot is one trigger element/component; its existing description is retained. */
export default defineComponent({
  name: 'AppTooltip',
  props: { text: { type: String, required: true } },
  setup(props, { slots }) {
    const focused = ref(false)
    const hovered = ref(false)
    const dismissed = ref(false)
    const visible = computed(() => !dismissed.value && (focused.value || hovered.value))
    const tooltipId = `ui-tooltip-${++nextId}`
    const anchor = ref<HTMLElement | null>(null)
    const tooltip = ref<HTMLElement | null>(null)
    const position = ref({ left: '12px', top: '12px' })
    async function place() {
      await nextTick()
      const trigger = anchor.value?.firstElementChild
      if (!visible.value || !trigger || !tooltip.value) return
      const from = trigger.getBoundingClientRect(), box = tooltip.value.getBoundingClientRect()
      const left = Math.max(12, Math.min((from.left + from.right - box.width) / 2, window.innerWidth - box.width - 12))
      const below = from.bottom + 6
      const top = below + box.height <= window.innerHeight - 12 ? below : from.top - box.height - 6
      position.value = { left: `${left}px`, top: `${Math.max(12, Math.min(top, window.innerHeight - box.height - 12))}px` }
    }
    function detach() { window.removeEventListener('resize', place); window.removeEventListener('scroll', place, true) }
    watch(visible, open => {
      detach()
      if (open) { window.addEventListener('resize', place); window.addEventListener('scroll', place, true); void place() }
    }, { flush: 'post' })
    watch(() => props.text, () => { if (visible.value) void place() })
    onBeforeUnmount(detach)
    return () => {
      const trigger = slots.default?.()[0]
      if (!trigger) return null
      const describedBy = [trigger.props?.['aria-describedby'], visible.value ? tooltipId : null].filter(Boolean).join(' ') || undefined
      return h('span', { ref: anchor, class: 'ui-tooltip-anchor' }, [
        cloneVNode(trigger, {
          'aria-describedby': describedBy,
          onFocus: () => {
            if (!focused.value) dismissed.value = false
            focused.value = true
          },
          onBlur: () => { focused.value = false },
          onMouseenter: () => {
            if (!hovered.value) dismissed.value = false
            hovered.value = true
          },
          onMouseleave: () => { hovered.value = false },
          onKeydown: (event: KeyboardEvent) => {
            if (event.key === 'Escape' && visible.value) {
              dismissed.value = true
              event.stopPropagation()
            }
          },
        }),
        visible.value && props.text ? h('span', { ref: tooltip, id: tooltipId, role: 'tooltip', class: 'ui-tooltip', style: position.value }, props.text) : null,
      ])
    }
  },
})
</script>
