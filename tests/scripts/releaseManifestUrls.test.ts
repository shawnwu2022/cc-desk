import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import { describe, expect, it } from 'vitest'

type VerifyUpdaterManifestUrls = (
  manifest: { platforms: Record<string, { url: string }> },
) => Promise<void>

const requireModule = createRequire(import.meta.url)
const { verifyUpdaterManifestUrls } = requireModule('../../scripts/verify-updater-manifest.js') as {
  verifyUpdaterManifestUrls: VerifyUpdaterManifestUrls
}

describe('release manifest URL verification', () => {
  it('ReleaseManifestUrls_AllAvailable_001', async () => {
    const server = createServer((_, response) => response.writeHead(302, { location: '/asset' }).end())
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
    const { port } = server.address() as { port: number }

    try {
      await expect(
        verifyUpdaterManifestUrls({
          platforms: { 'windows-x86_64': { url: `http://127.0.0.1:${port}/asset` } },
        }),
      ).resolves.toBeUndefined()
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => (error ? reject(error) : resolve())))
    }
  })

  it('ReleaseManifestUrls_Rejects404_002', async () => {
    const server = createServer((_, response) => response.writeHead(404).end())
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
    const { port } = server.address() as { port: number }

    try {
      await expect(
        verifyUpdaterManifestUrls({
          platforms: { 'windows-x86_64': { url: `http://127.0.0.1:${port}/missing` } },
        }),
      ).rejects.toThrow(/returns 404/)
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => (error ? reject(error) : resolve())))
    }
  })
})
