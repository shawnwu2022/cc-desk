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

The current native adapter still deliberately reports overall Unknown for a
tracked remote native run after output_reader_joined becomes true: waiter/retry,
master, command-owner and whole-tree retirement witnesses remain unavailable.
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
