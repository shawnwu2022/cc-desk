//! RSC1 facts only. Neither a wire reply nor permission to reconstruct a writer.
use crate::lifecycle_router::ResourceIdentity;
use cc_desk_remote_record::lifecycle::{self, Code, Kind, Request, State};
use cc_desk_remote_terminal::TerminalEpochs;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Applied,
    NotApplied,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fact {
    pub request: Request,
    pub resource: ResourceIdentity,
    pub backend: [u8; 32],
    pub digest: [u8; 32],
    pub before_size: (u16, u16),
    pub after_size: (u16, u16),
    pub before_terminal: TerminalEpochs,
    pub after_terminal: TerminalEpochs,
    pub after: State,
    pub outcome: Outcome,
}
impl Fact {
    fn validate(&self) -> Result<(), Code> {
        lifecycle::encode(&self.request)?;
        let r = &self.resource;
        let hex = |s: &str, n: usize| {
            s.len() == n
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let number = |s: &str| {
            s.parse::<u64>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == s)
        };
        if self.request.kind != Kind::Resize
            || !hex(&r.host, 64)
            || !hex(&r.session, 32)
            || r.run.is_empty()
            || r.run.len() > 128
            || r.run.chars().any(char::is_control)
            || number(&r.generation) != Some(self.request.state.generation)
            || number(&r.incarnation).is_none()
            || r.principal == [0; 32]
            || self.backend == [0; 32]
            || self.digest == [0; 32]
        {
            return Err(Code::Conflict);
        }
        let size = |(rows, cols): (u16, u16)| {
            rows > 0
                && usize::from(rows) <= cc_desk_remote_terminal::MAX_TERMINAL_ROWS
                && usize::from(cols) >= cc_desk_remote_terminal::MIN_TERMINAL_COLUMNS
                && usize::from(cols) <= cc_desk_remote_terminal::MAX_TERMINAL_COLUMNS
        };
        if !size(self.before_size) || !size(self.after_size) {
            return Err(Code::Malformed);
        }
        let requested = self.request.size.ok_or(Code::Malformed)?;
        if !size(requested) || u32::from(requested.0) * u32::from(requested.1) > 16384 {
            return Err(Code::Malformed);
        }
        let mut terminal = self.before_terminal;
        let mut after = self.request.state;
        match self.outcome {
            Outcome::Applied => {
                if self.after_size != requested {
                    return Err(Code::Conflict);
                }
                if self.before_size != self.after_size {
                    terminal.dimension_epoch =
                        terminal.dimension_epoch.checked_add(1).ok_or(Code::Stale)?;
                    terminal.structure_epoch =
                        terminal.structure_epoch.checked_add(1).ok_or(Code::Stale)?;
                    after.dimension = terminal.dimension_epoch;
                }
            }
            Outcome::NotApplied => {
                if self.after_size != self.before_size {
                    return Err(Code::Conflict);
                }
            }
        }
        if self.before_terminal.dimension_epoch != self.request.state.dimension
            || self.before_terminal.mode_epoch != self.request.state.mode
            || self.after_terminal != terminal
            || self.after != after
        {
            return Err(Code::Conflict);
        }
        Ok(())
    }
    pub(crate) fn encode_stored(&self) -> Result<Vec<u8>, Code> {
        self.validate()?;
        let request = lifecycle::encode(&self.request)?;
        let mut b = b"RSC1".to_vec();
        b.extend((request.len() as u32).to_be_bytes());
        b.extend(request);
        b.extend(self.backend);
        b.extend(self.digest);
        for s in [
            &self.resource.host,
            &self.resource.session,
            &self.resource.run,
            &self.resource.generation,
            &self.resource.incarnation,
        ] {
            b.extend((s.len() as u16).to_be_bytes());
            b.extend(s.as_bytes());
        }
        b.extend(self.resource.principal);
        b.push(match self.outcome {
            Outcome::Applied => 0,
            Outcome::NotApplied => 1,
        });
        for n in [
            self.before_size.0,
            self.before_size.1,
            self.after_size.0,
            self.after_size.1,
        ] {
            b.extend(n.to_be_bytes());
        }
        for n in [
            self.after.generation,
            self.after.fence,
            self.after.mode,
            self.after.dimension,
            self.after.sync,
            self.before_terminal.mode_epoch,
            self.before_terminal.dimension_epoch,
            self.before_terminal.structure_epoch,
            self.after_terminal.mode_epoch,
            self.after_terminal.dimension_epoch,
            self.after_terminal.structure_epoch,
        ] {
            b.extend(n.to_be_bytes());
        }
        if b.len() > 4096 {
            return Err(Code::Malformed);
        }
        Ok(b)
    }
    pub(crate) fn decode_stored(b: &[u8]) -> Result<Self, Code> {
        if b.len() > 4096 || !b.starts_with(b"RSC1") {
            return Err(Code::Malformed);
        }
        let mut c = Cursor(&b[4..]);
        let n = u32::from_be_bytes(c.array()?) as usize;
        let request = lifecycle::decode(c.take(n)?)?;
        let backend = c.array()?;
        let digest = c.array()?;
        let resource = ResourceIdentity {
            host: c.string()?,
            session: c.string()?,
            run: c.string()?,
            generation: c.string()?,
            incarnation: c.string()?,
            principal: c.array()?,
        };
        let outcome = match c.take(1)?[0] {
            0 => Outcome::Applied,
            1 => Outcome::NotApplied,
            _ => return Err(Code::Malformed),
        };
        let before_size = (c.u16()?, c.u16()?);
        let after_size = (c.u16()?, c.u16()?);
        let after = State {
            generation: c.u64()?,
            fence: c.u64()?,
            mode: c.u64()?,
            dimension: c.u64()?,
            sync: c.u64()?,
        };
        let before_terminal = TerminalEpochs {
            mode_epoch: c.u64()?,
            dimension_epoch: c.u64()?,
            structure_epoch: c.u64()?,
        };
        let after_terminal = TerminalEpochs {
            mode_epoch: c.u64()?,
            dimension_epoch: c.u64()?,
            structure_epoch: c.u64()?,
        };
        let f = Self {
            request,
            resource,
            backend,
            digest,
            before_size,
            after_size,
            before_terminal,
            after_terminal,
            after,
            outcome,
        };
        if !c.0.is_empty() || f.encode_stored()?.as_slice() != b {
            return Err(Code::Malformed);
        }
        Ok(f)
    }
}
struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Code> {
        let part = self.0.get(..n).ok_or(Code::Malformed)?;
        self.0 = &self.0[n..];
        Ok(part)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], Code> {
        self.take(N)?.try_into().map_err(|_| Code::Malformed)
    }
    fn u16(&mut self) -> Result<u16, Code> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, Code> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn string(&mut self) -> Result<String, Code> {
        let n = usize::from(self.u16()?);
        if n > 128 {
            return Err(Code::Malformed);
        }
        std::str::from_utf8(self.take(n)?)
            .map(str::to_owned)
            .map_err(|_| Code::Malformed)
    }
}
#[cfg(test)]
#[path = "lifecycle_resize_completion_tests.rs"]
pub(crate) mod tests;
