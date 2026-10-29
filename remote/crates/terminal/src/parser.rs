use vte::{Params, Parser, Perform};

/// Stable local reason for disabling remote terminal control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalSafeCode {
    OscOverflow,
    UnsupportedSequence,
    CombiningScalar,
    InputBatchExceeded,
    ReplyQueryBudgetExceeded,
    CanonicalEventBudgetExceeded,
    StateVersionExceeded,
}

/// A parser fault that makes the current terminal state unsafe for control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalFault {
    Degraded(TerminalSafeCode),
}

/// Persistent vte parser with fail-closed OSC overflow and OSC 52 filtering.
///
/// Replace this value only through an explicit local recovery path. It does not
/// expose a reset operation because an overflow makes the current emulator
/// continuation unsuitable for remote control.
#[derive(Default)]
pub struct TerminalParser {
    parser: Parser,
    degraded: Option<TerminalSafeCode>,
    blocked_osc52: u64,
}

impl TerminalParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one parser batch, stopping at the first OSC overflow.
    ///
    /// Bytes later in the same batch are ignored on overflow. Once degraded,
    /// all later calls fail without invoking `performer`.
    pub fn feed<P: Perform>(
        &mut self,
        performer: &mut P,
        bytes: &[u8],
    ) -> Result<(), TerminalFault> {
        if let Some(code) = self.degraded {
            return Err(TerminalFault::Degraded(code));
        }

        let mut filtered = FilteredPerformer {
            inner: performer,
            blocked_osc52: &mut self.blocked_osc52,
        };
        let consumed = self.parser.advance_until_osc_overflow(&mut filtered, bytes);

        if self.parser.take_osc_overflowed() {
            self.degraded = Some(TerminalSafeCode::OscOverflow);
            return Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow));
        }

        debug_assert_eq!(consumed, bytes.len());
        Ok(())
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.is_some()
    }

    /// Count of OSC 52 commands blocked before reaching the consumer.
    pub fn blocked_osc52_count(&self) -> u64 {
        self.blocked_osc52
    }

    pub fn has_pending_string_sequence(&self) -> bool {
        self.parser.has_pending_string_sequence()
    }

    pub fn take_string_sequence_entered(&mut self) -> bool {
        self.parser.take_string_sequence_entered()
    }
}

struct FilteredPerformer<'a, P> {
    inner: &'a mut P,
    blocked_osc52: &'a mut u64,
}

impl<P: Perform> Perform for FilteredPerformer<'_, P> {
    fn print(&mut self, c: char) {
        self.inner.print(c);
    }

    fn execute(&mut self, byte: u8) {
        self.inner.execute(byte);
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.inner.hook(params, intermediates, ignore, action);
    }

    fn put(&mut self, byte: u8) {
        self.inner.put(byte);
    }

    fn unhook(&mut self) {
        self.inner.unhook();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        if osc_command(params) == Some(52) {
            *self.blocked_osc52 = self.blocked_osc52.saturating_add(1);
        } else {
            self.inner.osc_dispatch(params, bell_terminated);
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.inner
            .csi_dispatch(params, intermediates, ignore, action);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.inner.esc_dispatch(intermediates, ignore, byte);
    }
}

fn osc_command(params: &[&[u8]]) -> Option<u16> {
    params.first()?.iter().try_fold(0u16, |value, byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value
            .checked_mul(10)?
            .checked_add(u16::from(byte.checked_sub(b'0')?))
    })
}
