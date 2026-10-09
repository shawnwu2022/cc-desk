#!/usr/bin/env node

// The legacy updater/OSS generator compatibility contract is retired.
// Forward to the current release policy tests and preserve their failure status.
require('./release.test.js')
