//! Where each participant sits on the Lab's map. Each takes a cell of its own, wide enough for its
//! token, its name and its line, in the order of its kind: miners, pools with their miners inside,
//! nodes that keep blocks, payers with the pile of waiting payments, and the bug exploiter. A row
//! that is full goes on below, so nothing lies on anything else at any width; the map grows
//! downwards as it fills.
use crate::local::lab_net::{Kind, Member};
use egui::{pos2, vec2, Pos2, Rect, Vec2};
use std::collections::BTreeMap;

pub const TOKEN: f32 = 20.0;
/// One participant: its token with room for a mark above, and two lines of words under it.
pub const CELL: Vec2 = vec2(124.0, 88.0);
/// Above the token, for the mark of what just happened to it.
pub const ABOVE: f32 = 10.0;
/// Between a token and its words.
pub const WORDS_GAP: f32 = 4.0;
/// The name and the line under a token or a ring.
pub const WORDS: f32 = 32.0;
/// The widest the words under a token may be.
pub const WORDS_WIDTH: f32 = CELL.x - 8.0;
/// The pool's shape on its ring's top edge.
pub const BADGE: f32 = 13.0;
const GAP: f32 = 8.0;
const MARGIN: f32 = 12.0;
/// Around the miners inside a ring.
const RING_PAD: f32 = 8.0;
/// The pile of waiting payments: two cells wide, and tall enough for its last line to wrap once.
const PILE: Vec2 = vec2(CELL.x * 2.0 + GAP, CELL.y + 16.0);

pub struct Places {
    /// Each token's centre; a pool's is its ring's.
    pub at: BTreeMap<u32, Pos2>,
    /// Each cell, and each pool's whole place: its shape, its ring and its words.
    pub cells: BTreeMap<u32, Rect>,
    pub rings: BTreeMap<u32, Rect>,
    pub pile: Option<Rect>,
    /// The height the map needs.
    pub height: f32,
}

enum Item {
    One(u32),
    Pool { id: u32, inside: Vec<u32>, columns: usize },
    Pile,
}

/// Kinds in the order their rows come.
fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::Miner | Kind::Attacker => 0,
        Kind::Pool => 1,
        Kind::Full | Kind::Pruned | Kind::Light => 2,
        Kind::Sender | Kind::Receiver | Kind::Spammer => 3,
        Kind::Exploiter => 4,
    }
}

fn in_pool(member: &Member, members: &[Member]) -> bool {
    member.pool.is_some_and(|pool| members.iter().any(|other| other.id == pool && other.kind == Kind::Pool))
}

fn rows_of(inside: usize, columns: usize) -> usize {
    inside.div_ceil(columns).max(1)
}

fn size(item: &Item) -> Vec2 {
    match item {
        Item::One(_) => CELL,
        Item::Pool { inside, columns, .. } => {
            let rows = rows_of(inside.len(), *columns) as f32;
            vec2(*columns as f32 * CELL.x + RING_PAD * 2.0, BADGE * 2.0 + rows * CELL.y + RING_PAD + WORDS_GAP + WORDS)
        }
        Item::Pile => PILE,
    }
}

/// The places for `members` on a map `width` wide, from its top left corner; with the pile of
/// waiting payments when `pile`.
pub fn places(members: &[Member], width: f32, pile: bool) -> Places {
    let room = (width - MARGIN * 2.0).max(CELL.x);
    let most_columns = (((room - RING_PAD * 2.0) / CELL.x).floor() as usize).max(1);
    let mut items: Vec<(u8, Item)> = Vec::new();
    for member in members.iter().filter(|member| !in_pool(member, members)) {
        let item = match member.kind {
            Kind::Pool => {
                let inside: Vec<u32> = members.iter().filter(|other| other.pool == Some(member.id)).map(|other| other.id).collect();
                let wanted = match inside.len() {
                    0 | 1 => 1,
                    2..=4 => 2,
                    5..=9 => 3,
                    _ => 4,
                };
                Item::Pool { id: member.id, inside, columns: wanted.min(most_columns) }
            }
            _ => Item::One(member.id),
        };
        items.push((rank(member.kind), item));
    }
    if pile {
        // Right after the payers, whose payments it holds.
        items.push((3, Item::Pile));
    }
    // A stable sort keeps the order they came in within a kind.
    items.sort_by_key(|(rank, _)| *rank);
    let mut rows: Vec<(f32, Vec<(Item, Vec2)>)> = Vec::new();
    let mut row: Vec<(Item, Vec2)> = Vec::new();
    let mut used = 0.0;
    for (_, item) in items {
        let size = size(&item);
        if !row.is_empty() && used + GAP + size.x > room {
            rows.push((used, std::mem::take(&mut row)));
            used = 0.0;
        }
        used += if row.is_empty() { size.x } else { GAP + size.x };
        row.push((item, size));
    }
    if !row.is_empty() {
        rows.push((used, row));
    }
    let mut places = Places { at: BTreeMap::new(), cells: BTreeMap::new(), rings: BTreeMap::new(), pile: None, height: 0.0 };
    let mut top = MARGIN;
    for (used, row) in rows {
        let tallest = row.iter().map(|(_, size)| size.y).fold(0.0, f32::max);
        let mut left = MARGIN + ((room - used) / 2.0).max(0.0);
        for (item, size) in row {
            let rect = Rect::from_min_size(pos2(left, top), size);
            put(&mut places, item, rect);
            left += size.x + GAP;
        }
        top += tallest + GAP;
    }
    places.height = top - GAP + MARGIN;
    places
}

/// Where a token stands in its cell.
pub fn token_in(cell: Rect) -> Pos2 {
    pos2(cell.center().x, cell.top() + ABOVE + TOKEN)
}

fn put(places: &mut Places, item: Item, rect: Rect) {
    match item {
        Item::One(id) => {
            places.at.insert(id, token_in(rect));
            places.cells.insert(id, rect);
        }
        Item::Pool { id, inside, columns } => {
            let rows = rows_of(inside.len(), columns);
            let ring = Rect::from_min_size(rect.min + vec2(0.0, BADGE), vec2(rect.width(), BADGE + rows as f32 * CELL.y + RING_PAD));
            places.at.insert(id, ring.center());
            places.cells.insert(id, rect);
            places.rings.insert(id, ring);
            let grid = ring.top() + BADGE;
            for (index, member) in inside.iter().enumerate() {
                let (column, row) = (index % columns, index / columns);
                // The last row stands centred when it is not full.
                let in_row = if row + 1 == rows { inside.len() - row * columns } else { columns };
                let shift = (columns - in_row) as f32 * CELL.x / 2.0;
                let cell = Rect::from_min_size(pos2(ring.left() + RING_PAD + shift + column as f32 * CELL.x, grid + row as f32 * CELL.y), CELL);
                places.at.insert(*member, token_in(cell));
                places.cells.insert(*member, cell);
            }
        }
        Item::Pile => places.pile = Some(rect),
    }
}

impl Places {
    /// The same places with the map's corner at `corner`.
    pub fn moved(mut self, corner: Vec2) -> Self {
        for at in self.at.values_mut() {
            *at += corner;
        }
        for cell in self.cells.values_mut().chain(self.rings.values_mut()) {
            *cell = cell.translate(corner);
        }
        self.pile = self.pile.map(|pile| pile.translate(corner));
        self
    }
}
