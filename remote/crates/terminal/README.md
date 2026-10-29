# Bounded terminal core: E04 incremental checkpoint

In the default feature profile, TerminalParser consumes the explicit vendored
vte0.15.0 path with std only; no registry fallback or ansi/log feature is enabled.
Official package checksum/licenses and patch provenance are in
remote/vendor/vte/CC-DESK-PROVENANCE.md. The default parser does not own a PTY.

TerminalQueries is a separate bounded, persistent reply-only syntax adapter. It
returns only typed Primary DA for complete ESC[c/ESC[0c, never arbitrary reply
payloads. All other queries remain unanswered; it does not model canonical cursor,
color or dimensions, and it does not change TerminalCore control eligibility.
It limits feeds to16384 bytes and queries to32/batch before returning anything;
oversize/OSC overflow/query overflow become sticky with their original safe code.
The Host private writer consumes these events under its own activation/rate/stop
checks and performs all actual IO. No clipboard/navigation/native event is executed.

The std parser budgets4096 OSC payload bytes before append, including otherwise
ignored controls/separators.4097 marks overflow, clears retained bytes, discards raw
input through BEL or ST, and never dispatches a truncated command. The adapter stops
at the overflow byte, ignores the rest of that feed and stays degraded; replacing it
requires explicit local recovery. Raw parser discard/resume is distinct from adapter
control eligibility. Original advance_until_terminated semantics are preserved; a
separate consumed-prefix API prevents an ANSI Processor sticky-fault busy loop.

OSC command52 numeric variants (52/052/0052) are blocked before the generic consumer;
53 and nondecimal4< pass through. This is protocol filtering, not a sandbox against
malicious trusted Perform implementations. No clipboard/URI/native API is called by
this crate. no_std remains upstream ArrayVec behavior, including its old truncation;
the4096 safety profile is only std. Passing no_std regressions does not certify that
different profile for Host use.

The default feature set remains this parser-only profile. The default-off
`experimental-emulator` feature adds the official `alacritty_terminal=0.26.0` crate.
Its dependency on vte0.15.0 is forced through the workspace crates.io patch to the
same vendored source above. `TerminalCore` validates all input with the bounded
parser before applying a batch to a persistent Alacritty `Term`. Construction and
resize reject zero geometry and anything above240 columns by100 rows before calling
Alacritty allocation code. This software-only slice uses `scrolling_history=0`.

The experimental input gate currently admits printable non-combining Unicode,
CR/LF, cursor-home CSI, bounded basic SGR colors/flags/reset, and exact alternate-buffer
CSI1049 enter/leave. SGR admits at most16 scalar parameters from the basic color/flag
whitelist; extended-color38/48, colon subparameters and private SGR remain rejected.
The admitted Alacritty paths update fixed-size cell colors/flags only. It uses the
patched vte parser's classified callbacks/state. Combining scalars, every OSC/DCS/
SOS/PM/APC string (including an unterminated introducer), and all other control
sequences fail the core into a sticky degraded state before the batch reaches
Alacritty. The final16-scalar-per-cell capability remains OPEN; this checkpoint
admits zero combining scalars rather than exposing Alacritty's unbounded vector.
Each `feed` is rejected before parser work above16384 bytes. A cumulative per-turn
budget and wall-clock/CPU watchdog remain OPEN.

The event sink drops every event without logging or retaining payloads. OSC52 is
also disabled in Alacritty configuration, terminal replies are counted and discarded,
and no PTY/native clipboard/URI opener/renderer/user account is connected.

This is NOT the full terminal emulator/state projection. Source review found
unbounded `CellExtra::zerowidth: Vec<char>`, owned title and hyperlink strings, and
variable title/hyperlink payloads in trace logging. Those paths are unreachable from
the admitted input profile, but their production budgets remain OPEN:16-scalar cell
support, bounded history/reflow,1KiB title,2KiB URI/256 links, reviewed DCS and
synchronized-update handling, full log/handler privacy review, per-turn byte/CPU
watchdog, fuzz/allocator evidence, mode/revision snapshots, renderer, and real CLI/
platform acceptance. `remote_control_eligible` is always false. Do not attach
untrusted real PTY streams based on this checkpoint.
