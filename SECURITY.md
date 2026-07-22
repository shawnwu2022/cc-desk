# Security Policy

## Supported Versions

Security fixes are applied to the latest released `0.x` version of CC Desk. Older releases may receive guidance, but are not guaranteed to receive patches.

## Reporting a Vulnerability

Do not open a public issue for a suspected vulnerability.

Use GitHub's Private Vulnerability Reporting feature for this repository. Include a clear description, affected version, impact, reproducible steps or a proof of concept, and any suggested mitigation. Redact API keys, access tokens, private project paths, session transcripts, and user data. Until a dedicated Code of Conduct contact is published, the same private channel also accepts reports titled `[conduct]`; maintainers classify those separately from technical vulnerabilities.

Maintainers aim to acknowledge valid reports within 7 days, provide an initial assessment within 14 days, and coordinate a fix and disclosure timeline based on severity and exploitability. These are targets, not guarantees.

## Scope

In scope are vulnerabilities in CC Desk source code, official release assets, GitHub Actions workflows, and the CC Desk updater channel.

Out of scope are Claude Code itself, third-party Providers, MCP servers, third-party plugins, user-managed operating systems, and credentials exposed outside CC Desk. Reports involving those systems may still receive safe configuration guidance when practical.

## Disclosure

Please give maintainers a reasonable opportunity to investigate and release a fix before public disclosure. Once fixed, maintainers may publish a GitHub Security Advisory and release note describing impact, affected versions, and remediation.