<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import VersionManagerApp from '../manager/VersionManagerApp.vue'
import { installManagerFixture } from './managerFixture'
import { blockedHostCalls } from './tauriStub'

const fixture = installManagerFixture()
const ready = ref(false)
onMounted(async () => { await nextTick(); ready.value = true })
onBeforeUnmount(fixture.dispose)
</script>

<template>
  <div :data-visual-ready="ready" :data-blocked-host-calls="blockedHostCalls"
    :data-manager-inspects="fixture.calls.inspects" :data-manager-confirms="fixture.calls.confirms" :data-manager-returns="fixture.calls.returns">
    <VersionManagerApp />
  </div>
</template>
