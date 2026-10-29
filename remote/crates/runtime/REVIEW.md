# E06 local ownership review

Independent reviewer integration_transport_review found two Important contracts:
resume-stage failure after execution could misleadingly become ResumeFailedReaped;
and root ExitStatus could be mistaken for whole-tree/old-IO quiescence. Both corrected.
Any resume anomaly remains OutcomeUnknown even after cleanup. Actual RED/GREEN used
first resume/marker execution then normal second enumeration finding multiple owned
threads; final log Some(1) is the FIRST resume, not proof of a second ResumeThread0.
No test thread-selection override remains. RootExitOnly now explicitly types the
try_wait/stop result; descendants and old IO may still need a separate barrier.

Minor evidence wording corrected:4096 unread output bytes do not prove physical pipe
capacity/fullness; Toolhelp elapsed guard is soft and cannot preempt Win32 calls.
PID reuse speculation was withdrawn after Microsoft PROCESS_INFORMATION contract
verification: held Child process handle retains process object/PID; thread owner is
checked after opening. Snapshot normal termination requires ERROR_NO_MORE_FILES.

Final independent disposition: no remaining findings block this local checkpoint.
Root reran full remote workspace:30/30, strict Clippy/fmt pass; runtime6/6 included.
Twenty lifecycle samples showed88->88 handles in the author's controlled run, not
100-cycle stress. Existing desktop/ConPTY/launch-policy code is unchanged. This does
not close PTY, Host crash/detach, Windows11/CLI, E08 admission or whole-tree gates.
