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

With `experimental-emulator`, explicit `TerminalCore::new_with_queries` adds a
restricted canonical profile for Primary DA, DSR5 and DSR6. Ordinary construction
still denies queries. `feed_with_queries` validates the full batch first, advances
the mature emulator only to each query prefix, and lends a consumed encoder to the
trusted backend callback. The callback must check its stop/fence/capability before
encoding. Responses use the locked Alacritty handlers, not sampled batch-tail or
hardcoded cursor state. Each response is capped at10 bytes for the bounded geometry;
each batch has at most32 queries and16384 input bytes. Other native events remain
denied. Callback failure/panic freezes the continuation so validator/canonical
divergence cannot reopen a later feed. This does not implement full VT, snapshots,
remote eligibility, resize authority or a PTY connection in this crate.
On a query-enabled core, ordinary `feed` is state consumption with replies dropped;
it has no query callback/reply-completeness guarantee. The Host canonical writer
exclusively uses `feed_with_queries`; callers must not mix the entry points to
claim that every terminal query was answered.

Only this explicit query profile also consumes exact ConPTY startup CSI1t as inert
(no window activation), and exact DECSET/DECRST1004 through the mature focus flag.
It explicitly declines optional Win32-input-mode9001 and retains legacy VT input;
it does not claim to have enabled the extension or complete KEY_EVENT fidelity.
The [Microsoft compatibility contract](https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md#terminals-that-dont-support-9001h)
permits unsupported terminals to ignore that always-issued startup request.
Other window/mode operations and multi/subparameter variants remain rejected.
No focus event is synthesized from output. Full key modifiers, key-up, IME and
input-mode publication/encoding remain separate acceptance requirements.

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
resize require at least2 columns (the locked emulator's minimum), reject zero rows
and anything above240 columns by100 rows before calling
Alacritty allocation code. This software-only slice uses `scrolling_history=0`.

The explicit `semantic_snapshot` API copies the complete active row-major grid,
Host-decided wide/spacer/wrap roles, finite colors/styles, cursor and buffer kind.
It does not use trimmed row_text, clone CellExtra or serialize the Term/parser.
Its no-Clone/no-Serialize view has at most24000 fixed-size cells and redacts payload
Debug. Unmodeled extras/flags/palette/modes or invalid cursor roles fail and freeze
the core; degraded state cannot publish a stale-looking valid view. A minimum-width
regression reproduced a one-column wide-character panic and resize hang before
the shared pre-allocation geometry guard was corrected. Raw cursor checks precede
renderer-like spacer normalization, with checked subtraction. The native PTY's
general size API is separate; canonical construction independently requires2 cols.

This first view is not an emulator recovery checkpoint, revision/dimension/mode
epoch, Host sync/barrier or wire DTO. Both buffers and parser continuation stay on
Host. The private Host writer now captures this view at its serialized offset/
revision barrier with an explicit shared two-snapshot budget; see Host README.
Unique HostOwner wiring,128-cell/32KiB encoding,reducer/renderer,mode/dimension/
sync authority and input eligibility remain subsequent integration work; this API
alone does not complete E05 or allow remote control.

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
