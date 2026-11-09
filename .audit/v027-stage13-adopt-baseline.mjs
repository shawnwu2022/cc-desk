import fs from 'node:fs'
import crypto from 'node:crypto'
const directory = 'tests/visual/__screenshots__/'
const evidence = '.audit/v027-stage13-visual/'
const actualPath = evidence + 'test-results/visual-verification/unified-workspace-archived-sessions-snapshot/archived-sessions-actual.png'
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex')
const actual = fs.readFileSync(actualPath)
const expectedHash = 'b44fc17fa12de582abbcdf2dc67cd77d69990e1870f1eefaffc91a5eace2dda7'
if (hash(actual) !== expectedHash) throw Error('REVIEWED_ACTUAL_CHANGED')
const oldHash = 'f198a94537b494d1bdff2e03a2d6841412005746e8b05d0585ffdcb3513eada8'
if (hash(fs.readFileSync(directory + 'archived-sessions.png')) !== oldHash) throw Error('PREVIOUS_BASELINE_CHANGED')
const names = fs.readdirSync(directory).filter(name => name.endsWith('.png'))
if (names.length !== 13) throw Error('BASELINE_INVENTORY_CHANGED')
for (const name of names.filter(name => name !== 'archived-sessions.png')) {
  const candidate = evidence + 'visual-evidence/candidate-baselines-unapproved/' + name
  if (hash(fs.readFileSync(directory + name)) !== hash(fs.readFileSync(candidate))) throw Error('OTHER_BASELINE_CHANGED: ' + name)
}
const manifestPath = directory + 'approval-manifest.json'
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
const entry = manifest.entries.find(entry => entry.name === 'archived-sessions.png')
if (entry.sha256 !== '72cebb531e7293567a0ce7cc935de313b494d4aa20f317486d4bda5efa90da25') throw Error('PREVIOUS_MANIFEST_CHANGED')
const { name: _name, ...priorCurrentRevision } = structuredClone(entry)
const provenance = {
  sourceHead: '07001406a2d0364a0d2a902ed16ce4dd630d0284',
  testedCommit: '31c4b4b3b1f4564e056a5a58b81a1c7dc3a3852f',
  workflowRunId: '42458148299', workflowRunAttempt: 1, artifactId: '13608292920',
  workflowRunUrl: 'https://github.com/shawnwu2022/cc-desk/actions/runs/42458148299',
  previousTrackedBaselineSha256: oldHash,
  previousManifestNote: 'The prior top-level entry hash differed from the tracked HEAD PNG. Both historical metadata and the exact pre-adoption tracked hash are retained.',
  uploaderArchiveSha256: 'c917fe3d28ae522cdbad2dc8b3625075c39dc152cd204286374513a480057505',
}
Object.assign(entry, {
  sha256: expectedHash, bytes: actual.length, reviewVerdict: 'approve',
  provenance: 'Exact inspected PR81 Linux actual PNG. Original verification remains RED; fresh no-update verification required.',
  zipEntry: actualPath.slice(evidence.length),
  pixelReview: {
    playwrightChangedPixels: 367,
    acceptedChange: 'Only the new permanent-deletion availability separator and collapsed heading below the three existing archive rows. Rows, icons and main view retain their previous appearance.',
    inspected: ['actual', 'expected', 'diff'], reviewers: ['root', 'cleanup_safety_design_review'],
    decision: 'accept', report: '.audit/v027-stage13-validation.md',
  },
  revisionProvenance: provenance, priorCurrentRevision,
})
manifest.permanentDeletion0270Rc1Revision = {
  ...provenance, reviewDate: '2026-11-08', adoptedImages: ['archived-sessions.png'], unchangedImages: 12,
  originalVerification: 'RED: 1 screenshot and 2 obsolete archive-control assertions; retained',
  finalNoUpdateVerification: 'pending fresh current-source Linux CI with maxDiffPixels=0; not Windows WebView2 or live CLI acceptance',
}
fs.copyFileSync(actualPath, directory + 'archived-sessions.png')
fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n')
console.log(JSON.stringify({ adopted: 'archived-sessions.png', sha256: expectedHash, bytes: actual.length, unchanged: 12 }))
