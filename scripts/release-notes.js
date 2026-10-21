const fs = require('fs')
const path = require('path')
const root = path.resolve(__dirname, '..')
const MAX_NOTES_LENGTH = 4000

function localizedNotes(value) {
  if (!value || typeof value.en !== 'string' || !value.en.trim() || value.en.length > MAX_NOTES_LENGTH || value.en.includes('\0')) {
    throw new Error('English release highlights are required and must fit the notes budget')
  }
  const notes = { en: value.en.trim() }
  if (value.zh !== undefined) {
    if (typeof value.zh !== 'string' || !value.zh.trim() || value.zh.length > MAX_NOTES_LENGTH || value.zh.includes('\0')) throw new Error('invalid Chinese release highlights')
    notes.zh = value.zh.trim()
  }
  return notes
}
function readNotes(version, data = JSON.parse(fs.readFileSync(path.join(root, 'docs/release-notes.json'), 'utf8'))) {
  if (data.schemaVersion !== 1 || !data.releases || typeof data.releases !== 'object') throw new Error('unsupported release highlights source')
  const key = String(version).replace(/^v/, '')
  if (key !== 'Unreleased' && !/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(key)) throw new Error('invalid release highlights version')
  return Object.hasOwn(data.releases, key) ? localizedNotes(data.releases[key]) : null
}
function renderDocument(data, language) {
  const chinese = language === 'zh'
  return `${chinese ? '# 主要更新日志' : '# Release highlights'}\n\n` +
    `${chinese ? '[English](RELEASE_NOTES.md)。' : '[简体中文](RELEASE_NOTES_CN.md).'} ` +
    `${chinese ? '仅记录主要用户更新；完整开发记录见 [CHANGELOG.md](CHANGELOG.md)。' : 'Major user-facing changes only. See [CHANGELOG.md](CHANGELOG.md) for the complete engineering history.'}\n\n` +
    `<!-- Generated from docs/release-notes.json by node scripts/release-notes.js docs. -->\n\n` +
    Object.keys(data.releases).map(version => {
      const notes = readNotes(version, data)
      return `## [${version}]\n\n${notes[language] ?? notes.en}\n`
    }).join('\n')
}
function composeReleaseNotes({ version, repository, sha, technical, data }) {
  const notes = readNotes(version, data)
  if (!notes || String(version).replace(/^v/, '') === 'Unreleased') throw new Error('versioned English release highlights are required before publication')
  if (!/^[-\w.]+\/[-\w.]+$/.test(repository ?? '') || !/^[a-f0-9]{40}$/.test(sha ?? '') || !technical?.trim()) throw new Error('release notes source binding is incomplete')
  // 完整披露原样保留，现有 Source/Validation policy 校验继续逐行读取。
  return `${notes.en}\n\n[简体中文](https://github.com/${repository}/blob/${sha}/RELEASE_NOTES_CN.md)\n\n` +
    `<details>\n<summary>Technical validation details</summary>\n\n${technical}${technical.endsWith('\n') ? '' : '\n'}\n</details>\n`
}
function main() {
  if (process.argv[2] === 'docs' && process.argv.length === 3) {
    const data = JSON.parse(fs.readFileSync(path.join(root, 'docs/release-notes.json'), 'utf8'))
    for (const [language, name] of [['en', 'RELEASE_NOTES.md'], ['zh', 'RELEASE_NOTES_CN.md']]) fs.writeFileSync(path.join(root, name), renderDocument(data, language))
    return
  }
  if (process.argv[2] !== 'compose') throw new Error('usage: release-notes.js docs | compose --tag <tag> --technical <file> --output <file>')
  const options = new Map()
  for (let index = 3; index < process.argv.length; index += 2) {
    const key = process.argv[index], value = process.argv[index + 1]
    if (!['--tag', '--technical', '--output'].includes(key) || !value || options.has(key)) throw new Error('invalid release notes argument')
    options.set(key, value)
  }
  if (options.size !== 3 || path.resolve(options.get('--technical')) === path.resolve(options.get('--output'))) throw new Error('separate technical and public notes files are required')
  const body = composeReleaseNotes({ version: options.get('--tag'), repository: process.env.GITHUB_REPOSITORY, sha: process.env.GITHUB_SHA,
    technical: fs.readFileSync(options.get('--technical'), 'utf8') })
  fs.writeFileSync(options.get('--output'), body)
}
module.exports = { localizedNotes, readNotes, renderDocument, composeReleaseNotes }
if (require.main === module) main()
