# D10 Windows Bash failure and correction

Observed CI #160 (35805799005), source 0b306b292cf719db266e611f4a34a42d3f94b50d, merge 831e01f011a831c75f181c67f728105188aa262f. Full Rust job 107006245694 was read. Compilation and Clippy passed; 433 library cases passed / 1 failed / 15 ignored. All D10 cases except Bash roundtrip passed, including Native, both PowerShell variants, both cmd variants, old PowerShell refusal, and the explicitly invoked environment-removal worker.

The real receiver showed `single'inside` losing its apostrophe and swallowing all following arguments into one word. CRT-style Windows command quoting is not MSYS incoming argument quoting: the unquoted apostrophe is syntax before the fixed exec wrapper can preserve `$@`. This is a real parser-boundary failure, not a fixture timeout or a reason to remove the special-character assertions.

Fix candidate: on Windows, encode all forwarded words using Bash's ANSI-C quoted hex-byte literals inside a fixed exec statement. Caller bytes never appear as shell syntax; no eval, external decoder, input truncation, or path rewriting of argv is used. On Unix retain direct positional argv for OS-string preservation. Both Windows shell and shim variants must pass the original exact-argv probe before this correction is accepted.

Official syntax reference read 2026-09-23: https://www.gnu.org/software/bash/manual/html_node/ANSI_002dC-Quoting.html

Also corrected the two rustfmt-only layouts from CI #160. The simultaneously added Windows length-limit tests are intentionally RED until their separate validation implementation; do not conflate that expected RED with the Bash correction's result.
