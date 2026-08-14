import { readonly, ref, watch, type Ref } from 'vue'

export function useStickyActivation(source: () => boolean): Readonly<Ref<boolean>> {
  const activated = ref(source())
  watch(source, (value) => {
    if (value) activated.value = true
  }, { flush: 'sync' })
  return readonly(activated)
}
