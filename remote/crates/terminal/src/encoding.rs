//! Borrowing encoder retains the caller's snapshot/budget lease. It neither
//! mints a sync/barrier nor grants eligibility; the Host supplies the stamp.
use crate::*;
use cc_desk_remote_record::projection as wire;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingError {
    Stamp,
    Codec,
    Budget,
}
pub struct SnapshotEncoder<'a> {
    view: &'a SemanticSnapshot,
    stamp: wire::Stamp,
    state: wire::State,
    minor: u16,
    next: usize,
    bytes: usize,
    failed: bool,
}
impl<'a> SnapshotEncoder<'a> {
    pub fn new(
        view: &'a SemanticSnapshot,
        stamp: wire::Stamp,
        minor: u16,
    ) -> Result<Self, EncodingError> {
        let e = view.epochs();
        if stamp.dimension_epoch != e.dimension_epoch.to_string()
            || stamp.mode_epoch != e.mode_epoch.to_string()
            || stamp.structure_epoch != e.structure_epoch.to_string()
        {
            return Err(EncodingError::Stamp);
        }
        let c = view.cursor();
        let state = wire::State {
            rows: view.rows(),
            columns: view.columns(),
            buffer: match view.buffer() {
                BufferKind::Normal => wire::Buffer::Normal,
                BufferKind::Alternate => wire::Buffer::Alternate,
            },
            cursor: wire::Cursor {
                row: c.row,
                column: c.column,
                blinking: c.blinking,
                shape: match c.shape {
                    SemanticCursorShape::Block => wire::CursorShape::Block,
                    SemanticCursorShape::Underline => wire::CursorShape::Underline,
                    SemanticCursorShape::Beam => wire::CursorShape::Beam,
                    SemanticCursorShape::HollowBlock => wire::CursorShape::HollowBlock,
                    SemanticCursorShape::Hidden => wire::CursorShape::Hidden,
                },
            },
            focus_reporting: view.focus_reporting(),
        };
        let this = Self {
            view,
            stamp,
            state,
            minor,
            next: 0,
            bytes: 0,
            failed: false,
        };
        // Validate the complete context before a caller can send even part zero.
        this.part(0, 1)?;
        Ok(this)
    }
    fn part(&self, start: usize, count: usize) -> Result<Vec<u8>, EncodingError> {
        let message = wire::Message {
            profile: 1,
            content: wire::Content::SnapshotPart(wire::SnapshotPart {
                stamp: self.stamp.clone(),
                state: self.state.clone(),
                start: start as u16,
                cells: self.view.cells()[start..start + count]
                    .iter()
                    .map(cell)
                    .collect(),
            }),
        };
        wire::encode(&message, self.minor).map_err(|_| EncodingError::Codec)
    }
}
impl Iterator for SnapshotEncoder<'_> {
    type Item = Result<Vec<u8>, EncodingError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.next == self.view.cells().len() {
            return None;
        }
        let count = wire::MAX_CELLS.min(self.view.cells().len() - self.next);
        let result = self.part(self.next, count).and_then(|bytes| {
            let total = self
                .bytes
                .checked_add(bytes.len())
                .filter(|n| *n <= MAX_SNAPSHOT_BYTES)
                .ok_or(EncodingError::Budget)?;
            self.bytes = total;
            self.next += count;
            Ok(bytes)
        });
        if result.is_err() {
            self.failed = true;
        }
        Some(result)
    }
}
fn color(c: SemanticColor) -> wire::Color {
    match c {
        SemanticColor::Palette(n) => wire::Color::Palette(n),
        SemanticColor::Rgb(r, g, b) => wire::Color::Rgb(r, g, b),
    }
}
fn cell(c: &SemanticCell) -> wire::Cell {
    let s = c.style;
    wire::Cell {
        scalar: c.scalar,
        role: match c.role {
            CellRole::Single => wire::Role::Single,
            CellRole::WideLead => wire::Role::WideLead,
            CellRole::WideSpacer => wire::Role::WideSpacer,
            CellRole::LeadingWideSpacer => wire::Role::LeadingWideSpacer,
        },
        wrap_end: c.wrap_end,
        foreground: color(c.foreground),
        background: color(c.background),
        style: wire::Style {
            bold: s.bold,
            dim: s.dim,
            italic: s.italic,
            inverse: s.inverse,
            hidden: s.hidden,
            strikeout: s.strikeout,
            underline: match s.underline {
                Underline::None => wire::Underline::None,
                Underline::Single => wire::Underline::Single,
                Underline::Double => wire::Underline::Double,
                Underline::Curly => wire::Underline::Curly,
                Underline::Dotted => wire::Underline::Dotted,
                Underline::Dashed => wire::Underline::Dashed,
            },
        },
    }
}
