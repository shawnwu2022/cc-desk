//! Immutable bounded active-grid view, not a serialized emulator or wire DTO.
use super::*;
use alacritty_terminal::term::{TermMode, cell::Flags};
use alacritty_terminal::vte::ansi::{Color, CursorShape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Normal,
    Alternate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellRole {
    Single,
    WideLead,
    WideSpacer,
    LeadingWideSpacer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticColor {
    Palette(u16),
    Rgb(u8, u8, u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Underline {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticStyle {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strikeout: bool,
    pub underline: Underline,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SemanticCell {
    pub scalar: char,
    pub role: CellRole,
    pub wrap_end: bool,
    pub foreground: SemanticColor,
    pub background: SemanticColor,
    pub style: SemanticStyle,
}
impl fmt::Debug for SemanticCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticCell")
            .field("scalar", &"<redacted>")
            .field("role", &self.role)
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticCursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticCursor {
    pub row: u16,
    pub column: u16,
    pub shape: SemanticCursorShape,
    pub blinking: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    Degraded,
    UnsupportedState,
}
/// No public constructor/mutable cells, Clone or Serialize. Payload is private
/// trusted-backend data; snapshots cannot authorize input or native effects.
#[derive(PartialEq, Eq)]
pub struct SemanticSnapshot {
    rows: u16,
    columns: u16,
    buffer: BufferKind,
    cursor: SemanticCursor,
    cells: Vec<SemanticCell>,
    focus_reporting: bool,
}
impl fmt::Debug for SemanticSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticSnapshot")
            .field("rows", &self.rows)
            .field("columns", &self.columns)
            .field("buffer", &self.buffer)
            .field("cursor", &self.cursor)
            .field("cells", &self.cells.len())
            .finish_non_exhaustive()
    }
}
impl SemanticSnapshot {
    pub fn rows(&self) -> u16 {
        self.rows
    }
    pub fn columns(&self) -> u16 {
        self.columns
    }
    pub fn buffer(&self) -> BufferKind {
        self.buffer
    }
    pub fn cursor(&self) -> SemanticCursor {
        self.cursor
    }
    pub fn cells(&self) -> &[SemanticCell] {
        &self.cells
    }
    pub fn focus_reporting(&self) -> bool {
        self.focus_reporting
    }
    pub fn cell(&self, row: usize, column: usize) -> Option<&SemanticCell> {
        if row >= usize::from(self.rows) || column >= usize::from(self.columns) {
            return None;
        }
        self.cells.get(row * usize::from(self.columns) + column)
    }
}
fn color(value: Color) -> SemanticColor {
    match value {
        Color::Named(named) => SemanticColor::Palette(named as u16),
        Color::Indexed(index) => SemanticColor::Palette(u16::from(index)),
        Color::Spec(rgb) => SemanticColor::Rgb(rgb.r, rgb.g, rgb.b),
    }
}
fn cell(value: &Cell) -> Result<SemanticCell, ProjectionError> {
    // No lossy flattening of future combining, underline-color or link state.
    if value.extra.is_some() {
        return Err(ProjectionError::UnsupportedState);
    }
    let flags = value.flags;
    let known = Flags::INVERSE
        | Flags::BOLD
        | Flags::ITALIC
        | Flags::ALL_UNDERLINES
        | Flags::WRAPLINE
        | Flags::WIDE_CHAR
        | Flags::WIDE_CHAR_SPACER
        | Flags::DIM
        | Flags::HIDDEN
        | Flags::STRIKEOUT
        | Flags::LEADING_WIDE_CHAR_SPACER;
    if flags.bits() & !known.bits() != 0 || (flags & Flags::ALL_UNDERLINES).bits().count_ones() > 1
    {
        return Err(ProjectionError::UnsupportedState);
    }
    let roles =
        flags & (Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    let role = if roles.is_empty() {
        CellRole::Single
    } else if roles == Flags::WIDE_CHAR {
        CellRole::WideLead
    } else if roles == Flags::WIDE_CHAR_SPACER {
        CellRole::WideSpacer
    } else if roles == Flags::LEADING_WIDE_CHAR_SPACER {
        CellRole::LeadingWideSpacer
    } else {
        return Err(ProjectionError::UnsupportedState);
    };
    let underline = if flags.contains(Flags::UNDERCURL) {
        Underline::Curly
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        Underline::Dotted
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        Underline::Dashed
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        Underline::Double
    } else if flags.contains(Flags::UNDERLINE) {
        Underline::Single
    } else {
        Underline::None
    };
    Ok(SemanticCell {
        scalar: value.c,
        role,
        wrap_end: flags.contains(Flags::WRAPLINE),
        foreground: color(value.fg),
        background: color(value.bg),
        style: SemanticStyle {
            bold: flags.contains(Flags::BOLD),
            dim: flags.contains(Flags::DIM),
            italic: flags.contains(Flags::ITALIC),
            inverse: flags.contains(Flags::INVERSE),
            hidden: flags.contains(Flags::HIDDEN),
            strikeout: flags.contains(Flags::STRIKEOUT),
            underline,
        },
    })
}
impl TerminalCore {
    pub fn semantic_snapshot(&mut self) -> Result<SemanticSnapshot, ProjectionError> {
        let result = self.checked_semantic_snapshot();
        if matches!(result, Err(ProjectionError::UnsupportedState)) {
            self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
        }
        result
    }
    fn checked_semantic_snapshot(&self) -> Result<SemanticSnapshot, ProjectionError> {
        if self.is_degraded() {
            return Err(ProjectionError::Degraded);
        }
        let rows = self.term.screen_lines();
        let columns = self.term.columns();
        validate_size(rows, columns).map_err(|_| ProjectionError::UnsupportedState)?;
        let mode = *self.term.mode();
        let allowed_modes = TermMode::default() | TermMode::ALT_SCREEN | TermMode::FOCUS_IN_OUT;
        if mode.bits() & !allowed_modes.bits() != 0
            || self.term.grid().display_offset() != 0
            || self.term.grid().history_size() != 0
            || (0..alacritty_terminal::term::color::COUNT)
                .any(|index| self.term.colors()[index].is_some())
        {
            return Err(ProjectionError::UnsupportedState);
        }
        // Validate raw cursor coordinates/roles before any upstream renderer
        // helper can index or subtract from an unchecked spacer position.
        let point = self.term.grid().cursor.point;
        let row = usize::try_from(point.line.0).map_err(|_| ProjectionError::UnsupportedState)?;
        if row >= rows || point.column.0 >= columns {
            return Err(ProjectionError::UnsupportedState);
        }
        let cursor_cell = self
            .cell(row, point.column.0)
            .ok_or(ProjectionError::UnsupportedState)?;
        let cursor_column = if cursor_cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            let previous = point
                .column
                .0
                .checked_sub(1)
                .ok_or(ProjectionError::UnsupportedState)?;
            if !self
                .cell(row, previous)
                .ok_or(ProjectionError::UnsupportedState)?
                .flags
                .contains(Flags::WIDE_CHAR)
            {
                return Err(ProjectionError::UnsupportedState);
            }
            previous
        } else {
            point.column.0
        };
        let style = self.term.cursor_style();
        let native_shape = if mode.contains(TermMode::SHOW_CURSOR) {
            style.shape
        } else {
            CursorShape::Hidden
        };
        let shape = match native_shape {
            CursorShape::Block => SemanticCursorShape::Block,
            CursorShape::Underline => SemanticCursorShape::Underline,
            CursorShape::Beam => SemanticCursorShape::Beam,
            CursorShape::HollowBlock => SemanticCursorShape::HollowBlock,
            CursorShape::Hidden => SemanticCursorShape::Hidden,
        };
        let mut cells = Vec::with_capacity(rows * columns);
        for row in 0..rows {
            for column in 0..columns {
                cells.push(cell(
                    self.cell(row, column)
                        .ok_or(ProjectionError::UnsupportedState)?,
                )?);
            }
        }
        Ok(SemanticSnapshot {
            rows: rows as u16,
            columns: columns as u16,
            buffer: if mode.contains(TermMode::ALT_SCREEN) {
                BufferKind::Alternate
            } else {
                BufferKind::Normal
            },
            cursor: SemanticCursor {
                row: row as u16,
                column: cursor_column as u16,
                shape,
                blinking: style.blinking,
            },
            cells,
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmodeled_cell_state_refuses_projection_and_freezes_the_core() {
        for case in 0..3 {
            let mut core = TerminalCore::new(3, 10).unwrap();
            let cell = &mut core.term.grid_mut()[Line(0)][Column(0)];
            match case {
                0 => cell.push_zerowidth('\u{301}'),
                1 => cell.flags.insert(Flags::from_bits_retain(0x8000)),
                _ => cell
                    .flags
                    .insert(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE),
            }
            assert_eq!(
                core.semantic_snapshot().unwrap_err(),
                ProjectionError::UnsupportedState
            );
            assert!(core.is_degraded());
            assert!(core.feed(b"later").is_err());
        }
    }
    #[test]
    fn malformed_first_column_cursor_spacer_returns_fault_instead_of_panicking() {
        let mut core = TerminalCore::new(3, 10).unwrap();
        core.term.grid_mut()[Line(0)][Column(0)]
            .flags
            .insert(Flags::WIDE_CHAR_SPACER);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.semantic_snapshot()));
        assert!(
            result.is_ok(),
            "unvalidated cursor role reached renderer subtraction"
        );
        assert_eq!(
            result.unwrap().unwrap_err(),
            ProjectionError::UnsupportedState
        );
        assert!(core.is_degraded());
    }
}
