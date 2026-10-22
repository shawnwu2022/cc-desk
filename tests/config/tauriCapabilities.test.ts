import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

type Permission = { commands?: { allow: string[]; deny: string[] }; permissions?: string[] }
type Manifest = { default_permission: Permission; permissions: Record<string, Permission>; permission_sets: Record<string, Permission> }
const manifests: Record<string, Manifest> = JSON.parse(readFileSync('src-tauri/gen/schemas/acl-manifests.json', 'utf8'))

// 使用实际生成的 ACL 展开 default，防止依赖升级悄悄扩大前端可调用面；不替代 WebView IPC 验收。
function commands(capability: string) {
  const allowed = new Set<string>(), denied = new Set<string>()
  const visit = (identifier: string) => {
    const split = identifier.lastIndexOf(':')
    const plugin = identifier.slice(0, split), name = identifier.slice(split + 1)
    const manifest = manifests[plugin]
    expect(manifest, identifier).toBeDefined()
    const permission = name === 'default' ? manifest.default_permission : manifest.permissions[name] ?? manifest.permission_sets[name]
    expect(permission, identifier).toBeDefined()
    for (const command of permission.commands?.allow ?? []) allowed.add(`${plugin}:${command}`)
    for (const command of permission.commands?.deny ?? []) denied.add(`${plugin}:${command}`)
    for (const child of permission.permissions ?? []) visit(child.includes(':') ? child : `${plugin}:${child}`)
  }
  const config = JSON.parse(readFileSync(`src-tauri/capabilities/${capability}.json`, 'utf8'))
  for (const identifier of config.permissions) visit(identifier)
  return new Set([...allowed].filter(command => !denied.has(command)))
}

it('Capabilities_UpgradeKeepsMainAuthority_001', () => {
  const main = commands('default')
  for (const command of ['core:app:version', 'core:window:is_visible', 'core:window:set_title', 'core:resources:close']) expect(main.has(command), command).toBe(true)
  for (const command of ['core:app:supports_multiple_windows', 'core:window:activity_name', 'core:window:scene_identifier', 'core:webview:create_webview', 'core:window:create']) expect(main.has(command), command).toBe(false)
})

it('Capabilities_ManagerRemainsIsolated_002', () => {
  expect([...commands('version-manager')]).toEqual([])
})
