use crate::{TerminalFault, TerminalParser, TerminalSafeCode};
use vte::{Params, Perform};

/// Stateless queries only. No cursor/size/color reply is supported by this profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalQuery {
    PrimaryDeviceAttributes,
}

/// Persistent bounded syntax adapter; not a canonical display emulator or grant.
#[derive(Default)]
pub struct TerminalQueries {
    parser: TerminalParser,
    degraded: Option<TerminalSafeCode>,
}

impl TerminalQueries {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<TerminalQuery>, TerminalFault> {
        if let Some(code) = self.degraded {
            return Err(TerminalFault::Degraded(code));
        }
        if bytes.len() > 16384 {
            self.degraded = Some(TerminalSafeCode::InputBatchExceeded);
            return Err(TerminalFault::Degraded(
                TerminalSafeCode::InputBatchExceeded,
            ));
        }
        let mut sink = Queries::default();
        if let Err(error @ TerminalFault::Degraded(code)) = self.parser.feed(&mut sink, bytes) {
            self.degraded = Some(code);
            return Err(error);
        }
        if sink.overflow {
            self.degraded = Some(TerminalSafeCode::ReplyQueryBudgetExceeded);
            return Err(TerminalFault::Degraded(
                TerminalSafeCode::ReplyQueryBudgetExceeded,
            ));
        }
        Ok(sink.items)
    }
}

#[derive(Default)]
struct Queries {
    items: Vec<TerminalQuery>,
    overflow: bool,
}
impl Perform for Queries {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if !ignore && intermediates.is_empty() && action == 'c' && params.iter().eq([&[0][..]]) {
            if self.items.len() == 32 {
                self.overflow = true;
            } else {
                self.items.push(TerminalQuery::PrimaryDeviceAttributes);
            }
        }
    }
}
