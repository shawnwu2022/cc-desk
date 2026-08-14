import { ref } from 'vue'
import { describe, expect, it } from 'vitest'
import { useStickyActivation } from '@/composables/useStickyActivation'

describe('useStickyActivation', () => {
  // source 首次变 true 后 activated 置 true，之后 source 回 false 仍保持 true（粘性）。
  it('StickyActivation_ActivatesOnce_001', () => {
    const source = ref(false)
    const activated = useStickyActivation(() => source.value)

    expect(activated.value).toBe(false)
    source.value = true
    expect(activated.value).toBe(true)
    source.value = false
    expect(activated.value).toBe(true)
  })

  // source 初始即 true 时，activated 初始即为 true。
  it('StickyActivation_InitialState_002', () => {
    const source = ref(true)
    expect(useStickyActivation(() => source.value).value).toBe(true)
  })
})
