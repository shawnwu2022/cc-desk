# D10 boundary review: command and environment size

Author review found an unguarded parser boundary in the first implementation candidate: cmd can ignore inherited environment values beyond its length limit, and Windows command-line quoting/PowerShell data transport can expand the final command beyond CreateProcessW's limit. Rejecting only metacharacters does not prevent this form of silent loss.

Sources read 2026-09-23:
- https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/command-line-string-limitation
- https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
- https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing

This commit adds four failure tests for the missing validation and one positive guard showing that cmd's tighter environment restriction must not affect Native launches. Tests do not execute oversized commands, print resolved values, or alter native config. Observe these four RED failures before implementing the checks.

Ruling: validate the encoded/quoted command length, not only the raw argv sum. Count UTF-16 code units on Windows including spaces, quoting and the final terminator. Cmd may impose stricter limits; it must not silently remove/truncate environment variables. No fallback to another interpreter and no rewriting of argv are acceptable substitutes.

Status: tests submitted before the boundary implementation; not GREEN or complete D10 acceptance.
