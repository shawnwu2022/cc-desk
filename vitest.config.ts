import { defineConfig } from 'vitest/config'
import viteConfig from './vite.config'

const nodeTests = [
  'tests/designTokens.test.ts',
  'tests/productBoundary.test.ts',
  'tests/config/csp.test.ts',
  'tests/config/manualChunks.test.ts',
  'tests/config/packageVersion.test.ts',
  'tests/config/restartLifetimeDiagnostics.test.ts',
  'tests/config/rustToolchainWorkflow.test.ts',
  'tests/config/terminalThemes.test.ts',
  'tests/config/versionHistoryContracts.test.ts',
  'tests/config/versionManagerIsolation.test.ts',
  'tests/config/versionManagerReentryIsolation.test.ts',
  'tests/config/versionManagerVisualBoundary.test.ts',
  'tests/native-cli/d20IdentityFreeze.test.ts',
  'tests/native-cli/d21OsBridge.test.ts',
  'tests/native-cli/realCliCertify.test.ts',
  'tests/native-cli/realCliEvidence.test.ts',
  'tests/utils/attentionSummary.test.ts',
  'tests/utils/displayName.test.ts',
  'tests/utils/json.test.ts',
  'tests/utils/nativeErrorCode.test.ts',
  'tests/utils/nativeRawArgv.test.ts',
  'tests/utils/projectGroupOrder.test.ts',
  'tests/utils/ptyIndex.test.ts',
  'tests/utils/relativeTime.test.ts',
  'tests/utils/rendererRegistry.test.ts',
  'tests/utils/sessionDot.test.ts',
  'tests/utils/sessionPresentation.test.ts',
  'tests/utils/updateErrors.test.ts',
  'tests/utils/updatePolicy.test.ts',
  'tests/utils/userError.test.ts',
]
const processTests = [
  'tests/config/testProjects.test.ts',
  'tests/config/visualEvidence.test.ts',
  'tests/config/visualFixtureBoundary.test.ts',
  'tests/config/visualWorkflow.test.ts',
  'tests/native-cli/baseline.test.ts',
  'tests/native-cli/finalAdversarial.test.ts',
  'tests/native-cli/observerReporter.test.ts',
  'tests/native-cli/oracle.test.ts',
  'tests/native-cli/probe.test.ts',
  'tests/native-cli/realCliCollector.test.ts',
  'tests/native-cli/realCliCommand.test.ts',
  'tests/native-cli/realCliRunner.test.ts',
  'tests/scripts/generateUpdaterManifest.test.ts',
  'tests/scripts/releaseManifestUrls.test.ts',
]
const stressTests = ['tests/stores/unifiedStress.test.ts']

export default defineConfig(env => ({
  ...viteConfig(env),
  test: {
    globals: true,
    projects: [
      { extends: true, test: { name: 'fast-node', environment: 'node', include: nodeTests } },
      // 未分类的新文件默认保留 DOM 环境，完整入口仍会自动发现它们。
      { extends: true, test: {
        name: 'fast-dom', environment: 'jsdom', include: ['tests/**/*.test.ts'],
        exclude: ['tests/visual/**', ...nodeTests, ...processTests, ...stressTests],
        setupFiles: ['tests/test-setup.ts'],
      } },
      { extends: true, test: { name: 'process', environment: 'node', include: processTests } },
      { extends: true, test: {
        name: 'stress', environment: 'jsdom', include: stressTests, setupFiles: ['tests/test-setup.ts'],
      } },
    ],
  },
}))
