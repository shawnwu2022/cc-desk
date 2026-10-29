//! Semantic-grid trial profile. Decode is bounded syntax validation, never peer
//! authentication, snapshot installation, ACK eligibility or input authority.
use serde::{Deserialize, Serialize};
use std::fmt;
pub const MAX_CELLS: usize = 128;
pub const MAX_PAYLOAD: usize = 32768;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Version,
    Json,
    Field,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub profile: u16,
    #[serde(flatten)]
    pub content: Content,
}
impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectionMessage")
            .field("profile", &self.profile)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "message", content = "body")]
pub enum Content {
    SnapshotPart(SnapshotPart),
    Delta(Delta),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub host: String,
    pub session: String,
    pub run: String,
    pub generation: String,
    pub incarnation: String,
}
/// All counters use canonical decimal strings to preserve u64 across JS runtimes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    pub target: Target,
    pub binding: String,
    pub barrier: String,
    pub revision: String,
    pub dimension_epoch: String,
    pub mode_epoch: String,
    pub structure_epoch: String,
    pub sync_epoch: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Buffer {
    Normal,
    Alternate,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub row: u16,
    pub column: u16,
    pub shape: CursorShape,
    pub blinking: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub rows: u16,
    pub columns: u16,
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub focus_reporting: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Single,
    WideLead,
    WideSpacer,
    LeadingWideSpacer,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Palette(u16),
    Rgb(u8, u8, u8),
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Underline {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Style {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strikeout: bool,
    pub underline: Underline,
}
impl From<Style> for u16 {
    fn from(s: Style) -> Self {
        u16::from(s.bold)
            | u16::from(s.dim) << 1
            | u16::from(s.italic) << 2
            | u16::from(s.inverse) << 3
            | u16::from(s.hidden) << 4
            | u16::from(s.strikeout) << 5
            | (match s.underline {
                Underline::None => 0,
                Underline::Single => 1,
                Underline::Double => 2,
                Underline::Curly => 3,
                Underline::Dotted => 4,
                Underline::Dashed => 5,
            }) << 6
    }
}
impl TryFrom<u16> for Style {
    type Error = &'static str;
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        if n & !511 != 0 {
            return Err("style bits");
        }
        let underline = match n >> 6 {
            0 => Underline::None,
            1 => Underline::Single,
            2 => Underline::Double,
            3 => Underline::Curly,
            4 => Underline::Dotted,
            5 => Underline::Dashed,
            _ => return Err("underline"),
        };
        Ok(Self {
            bold: n & 1 != 0,
            dim: n & 2 != 0,
            italic: n & 4 != 0,
            inverse: n & 8 != 0,
            hidden: n & 16 != 0,
            strikeout: n & 32 != 0,
            underline,
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    #[serde(rename = "c")]
    pub scalar: char,
    #[serde(rename = "r")]
    pub role: Role,
    #[serde(rename = "w")]
    pub wrap_end: bool,
    #[serde(rename = "f")]
    pub foreground: Color,
    #[serde(rename = "b")]
    pub background: Color,
    #[serde(rename = "s")]
    pub style: Style,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPart {
    pub stamp: Stamp,
    pub state: State,
    pub start: u16,
    #[serde(deserialize_with = "bounded_cells")]
    pub cells: Vec<Cell>,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub index: u16,
    pub cell: Cell,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub stamp: Stamp,
    pub state: State,
    pub base: String,
    #[serde(deserialize_with = "bounded_cells")]
    pub changes: Vec<Change>,
}
fn bounded_cells<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most128 cells")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut a: A,
        ) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(cell) = a.next_element()? {
                if v.len() == MAX_CELLS {
                    return Err(serde::de::Error::custom("cell limit"));
                }
                v.push(cell);
            }
            Ok(v)
        }
    }
    d.deserialize_seq(Bounded(std::marker::PhantomData))
}
pub fn decimal(s: &str) -> Result<u64, Error> {
    if s.is_empty()
        || s.len() > 20
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(Error::Field);
    }
    s.parse().map_err(|_| Error::Field)
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn stamp(s: &Stamp) -> Result<(), Error> {
    let t = &s.target;
    if !hex(&t.host, 64)
        || !hex(&t.session, 32)
        || !hex(&s.binding, 64)
        || !hex(&s.barrier, 32)
        || t.run.is_empty()
        || t.run.len() > 128
        || t.run.chars().any(char::is_control)
    {
        return Err(Error::Field);
    }
    for n in [
        &t.generation,
        &t.incarnation,
        &s.revision,
        &s.dimension_epoch,
        &s.mode_epoch,
        &s.structure_epoch,
        &s.sync_epoch,
    ] {
        decimal(n)?;
    }
    Ok(())
}
fn state(s: &State) -> Result<usize, Error> {
    if !(1..=100).contains(&s.rows)
        || !(2..=240).contains(&s.columns)
        || s.cursor.row >= s.rows
        || s.cursor.column >= s.columns
    {
        return Err(Error::Field);
    }
    Ok(usize::from(s.rows) * usize::from(s.columns))
}
fn cell(c: &Cell) -> Result<(), Error> {
    if c.scalar.is_control() {
        return Err(Error::Field);
    }
    for color in [c.foreground, c.background] {
        if matches!(color,Color::Palette(n) if n>268) {
            return Err(Error::Field);
        }
    }
    Ok(())
}
fn validate(m: &Message, minor: u16) -> Result<(), Error> {
    if minor > 1 || m.profile != 1 {
        return Err(Error::Version);
    }
    match &m.content {
        Content::SnapshotPart(p) => {
            stamp(&p.stamp)?;
            let total = state(&p.state)?;
            if p.cells.is_empty()
                || p.cells.len() > MAX_CELLS
                || usize::from(p.start) + p.cells.len() > total
            {
                return Err(Error::Field);
            }
            for c in &p.cells {
                cell(c)?;
            }
        }
        Content::Delta(d) => {
            stamp(&d.stamp)?;
            let total = state(&d.state)?;
            if decimal(&d.base)? >= decimal(&d.stamp.revision)?
                || d.changes.len() > MAX_CELLS
                || d.changes.windows(2).any(|w| w[0].index >= w[1].index)
            {
                return Err(Error::Field);
            }
            for c in &d.changes {
                if usize::from(c.index) >= total {
                    return Err(Error::Field);
                }
                cell(&c.cell)?;
            }
        }
    }
    Ok(())
}
pub fn decode(payload: &[u8], negotiated_minor: u16) -> Result<Message, Error> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::Length);
    }
    if negotiated_minor > 1 {
        return Err(Error::Version);
    }
    let message = serde_json::from_slice(payload).map_err(|_| Error::Json)?;
    validate(&message, negotiated_minor)?;
    Ok(message)
}
pub fn encode(message: &Message, negotiated_minor: u16) -> Result<Vec<u8>, Error> {
    validate(message, negotiated_minor)?;
    let payload = serde_json::to_vec(message).map_err(|_| Error::Json)?;
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::Length);
    }
    Ok(payload)
}
