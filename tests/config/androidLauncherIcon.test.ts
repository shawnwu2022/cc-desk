import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

it('AndroidIcon_BothBuildsUseDesktopArtworkWithoutASecondAsset_001', () => {
  const read = (f: string) => readFileSync('android-client/' + f, 'utf8')
  expect(read('AndroidManifest.xml')).toContain('android:icon="@mipmap/ccdesk_launcher"')
  expect(read('prepare-icon-resources.ps1')).toContain('src-tauri/icons/source/png/ccdesk-256.png')
  for (const script of ['build-development.ps1', 'build-production.ps1']) {
    expect(read(script)).toContain('prepare-icon-resources.ps1')
    expect(read(script)).toContain("'-S',$iconResources")
  }
})
