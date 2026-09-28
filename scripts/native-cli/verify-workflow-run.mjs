#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const COMMIT = /^[0-9a-f]{40}$/

function fail(code) {
  throw new Error(code)
}

export function verifyWorkflowRun(run, {
  expectedPath,
  expectedSha,
  allowedEvents,
}) {
  if (!run || typeof run !== 'object' || Array.isArray(run)) fail('RUN_PROVENANCE_INVALID')
  if (typeof expectedPath !== 'string' || !expectedPath.startsWith('.github/workflows/')) {
    fail('RUN_PROVENANCE_PATH_INVALID')
  }
  if (!COMMIT.test(expectedSha ?? '')) fail('RUN_PROVENANCE_SHA_INVALID')
  if (!Array.isArray(allowedEvents) || allowedEvents.length === 0) {
    fail('RUN_PROVENANCE_EVENTS_INVALID')
  }

  if (run.path !== expectedPath) fail('RUN_PROVENANCE_WORKFLOW_MISMATCH')
  if (run.head_sha !== expectedSha) fail('RUN_PROVENANCE_HEAD_MISMATCH')
  if (run.status !== 'completed' || run.conclusion !== 'success') {
    fail('RUN_PROVENANCE_NOT_SUCCESSFUL')
  }
  if (!allowedEvents.includes(run.event)) fail('RUN_PROVENANCE_EVENT_MISMATCH')

  return {
    status: 'PASS',
    runId: run.id,
    path: run.path,
    headSha: run.head_sha,
    event: run.event,
  }
}

function parse(argv) {
  const values = {}
  const events = []
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (key === '--event') {
      if (index + 1 >= argv.length) fail('RUN_PROVENANCE_OPTION_INVALID')
      events.push(argv[++index])
      continue
    }
    if (!['--run-json', '--workflow-path', '--head-sha'].includes(key)) {
      fail('RUN_PROVENANCE_OPTION_INVALID')
    }
    if (index + 1 >= argv.length || values[key]) fail('RUN_PROVENANCE_OPTION_INVALID')
    values[key] = argv[++index]
  }
  for (const key of ['--run-json', '--workflow-path', '--head-sha']) {
    if (!values[key]) fail('RUN_PROVENANCE_OPTION_REQUIRED')
  }
  if (events.length === 0) fail('RUN_PROVENANCE_OPTION_REQUIRED')
  return { values, events }
}

function main() {
  try {
    const { values, events } = parse(process.argv.slice(2))
    const run = JSON.parse(readFileSync(resolve(values['--run-json']), 'utf8'))
    const result = verifyWorkflowRun(run, {
      expectedPath: values['--workflow-path'],
      expectedSha: values['--head-sha'],
      allowedEvents: events,
    })
    process.stdout.write(`${JSON.stringify(result)}\n`)
  } catch (error) {
    const reason = error instanceof Error && /^[A-Z0-9_]+$/.test(error.message)
      ? error.message
      : 'RUN_PROVENANCE_FAILED'
    process.stderr.write(`[run-provenance-error] ${reason}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
