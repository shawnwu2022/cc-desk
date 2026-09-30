<script lang="ts">
import { cloneVNode, computed, defineComponent, h, ref } from 'vue'

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
    return () => {
      const trigger = slots.default?.()[0]
      if (!trigger) return null
      const describedBy = [trigger.props?.['aria-describedby'], visible.value ? tooltipId : null].filter(Boolean).join(' ') || undefined
      return h('span', { class: 'ui-tooltip-anchor' }, [
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
        visible.value && props.text ? h('span', { id: tooltipId, role: 'tooltip', class: 'ui-tooltip' }, props.text) : null,
      ])
    }
  },
})
</script>
