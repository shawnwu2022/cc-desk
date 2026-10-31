# Experimental native Host lifecycle

`remote-host` is an opt-in Cargo feature, excluded from the default build. It
includes the existing `remote-client-ipc` feature and the local Host dependency.
The application service starts Locked: there is no JavaScript owner, identity,
credential, pairing or native-provider creation API. Enabling this feature does
not make remote sessions usable or certify a mobile/CLI/platform combination.

The native Host facade retains each original pending rebind and its original
`RunningChannel`. An observation token does not own the operation. Dropping or
timing out an observer cannot replay it. Installation facts are cached only after
the actor's actual success reply; they grant neither Data access nor snapshot ACK
or input-window eligibility. Revoked authority prevents disclosure.

Phone disconnection closes that exact channel authority, without sealing the
session writer or stopping the user's process. Application shutdown instead seals
admission, requests all owned stops, and concurrently observes original network
tasks, pending installs, writer joins and native stop work. Both desktop close and
explicit exit use one retained waiter and the first exit code; handoff maintenance
refusal takes precedence. There is no timeout-to-success or automatic retry.

`WriterRetirement::Joined` proves only the owned writer thread joined. Network
join proves its original task was awaited, not normal protocol completion:
`RunningChannel::wait_closed` folds task panic into `Closed`. Root process exit,
EOF, an empty active cache or the old five-second shutdown return do not prove
whole PTY/reader/master/descendant or command-owner retirement.

In Host-enabled builds, the existing output reader's original JoinHandle is held
in one fixed slot, published before adoption completes, and actually joined by
the retained shutdown observer. EOF/output callbacks do not prove thread exit.
The aggregate remains Pending while that reader is Publishing/Running, even if
root/stop facts are already Unknown, so the application cannot stop observing it
early. A thread panic stays Unknown, not Joined; a second reservation cannot
replace an existing handle. No hard preemption or join deadline is promised.

The native adapter also retains each original waiter/retry thread handle in a
per-run bounded set in Host-enabled builds (64 unjoined handles maximum). Only
finished original handles are joined; panic stays sticky Unknown while other
live threads remain Pending. A full set rejects creation without losing custody;
an explicit stop/shutdown may retry after finished handles are collected. This
is a local custody bound, not a global capacity/DoS guarantee. Default-off
non-test thread spawning behavior is unchanged.

The final registry-retired check and new-thread publication share the custody
lock. A late starter cannot create a thread after terminal joins. Observers use
try_lock and report Pending during publication; no deadline for thread creation,
join, or native calls is promised. wait_workers_joined requires successful real
joins plus registry retirement, not waiter_running=false or a lifecycle callback.

Host-enabled creation wraps the three original Rust I/O owners with passive
destructor-return tracking. The transferred reader stays tracked until its actual
Drop, not take_reader or EOF. master and writer use Deref to the original objects;
the master-to-writer-to-untransferred-reader order remains unchanged. No added
flush, waiting, native close policy, thread or handle duplication is introduced.
The reader wrapper allocation occurs before child creation. Default-off non-test
builds retain the original unwrapped types.

SupervisedRun captures a token containing only shared atomic status; it cannot
keep the I/O objects alive or change their last release. A completion bit is set
only after the corresponding underlying Drop returns normally. An unwind guard
records failure only, never successful return. Aggregate observation stays
Pending while any original owner is nonterminal, then Unknown if any failed.
original_io_drop_returned proves only that these Rust destructors returned; it
does not prove kernel close success, console-process exit, thread join or tree
cleanup. A blocked destructor remains pending; no hard deadline is claimed.

The current native adapter still deliberately reports overall Unknown for a
tracked remote native run after output_reader_joined, wait_workers_joined and
original_io_drop_returned become true: kernel/console, command-owner and
whole-tree retirement witnesses remain unavailable.
io_threads_joined therefore stays false. Unknown prevents application exit in
this opt-in mode and keeps custody. Default-off production reader behavior is
unchanged; no new process, worker framework or stop policy is introduced.

Remaining admission gates: a real authorized same-run writer/provider factory
(the facade currently creates an empty owner), complete native retirement proof,
authenticated peer-install bootstrap, actual WebView/CLI/phone integration,
multi-Host behavior and capacity/recovery qualification. No arbitrary peer/fence
constructor, production database migration, CLI version restriction, network
listener or deployment is introduced here. The staged native adoption seam
currently admits one exact live supervisor/run scope and is not a product
capacity promise.

The ordinary desktop paths now converge on the original `OwnedPty` mutation
owner: `NativeRuntime::input_commit` and `input_protocol` use
`RunAccess::with_writer`; `NativeRuntime::resize` uses `RunAccess::resize`.
Original writer and master handles share one mutation mutex. A logical frame
(including flush) excludes resize, while reader and root-stop locks remain
independent. Exact document/run/resource identity and Running phase are checked
again after waiting; Exited resources remain available for snapshot/output drain,
but cannot admit new write or resize effects. Revocation after admission does not
roll back an already executing effect. Existing input receipts/no-replay and
nonzero desktop dimensions remain unchanged.

The resize IPC awaits a blocking task just like the existing input paths, so
waiting for a logical write does not synchronously occupy an async worker.
Dropping the awaiting future does not cancel or replay an admitted native effect;
the original blocking task retains the same access and performs its lock-time
authorization check. No timeout is turned into a successful resize.

This is local serialization, not remote ownership transfer. It promises neither
FIFO among not-yet-admitted calls nor a deadline/preemption for blocking native
write or resize. The authenticated remote actor is still not installed as this
owner, and no parallel remote writer is exposed. A production provider must route
or reject the desktop authority at its actual handoff; this patch alone does not
solve that gate or complete PTY/Job retirement.
