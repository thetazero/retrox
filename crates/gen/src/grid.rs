//! A uniform 2D grid over the galactic plane for "what's near here" queries
//! during generation. The disc is thin, so bucketing by x/y is enough.

use retrox_sim::GalaxyPos;

pub struct Grid {
    cell: f64,
    origin: f64,
    width: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    /// A grid covering a square of half-size `extent` around the origin.
    pub fn new(extent: f64, cell: f64) -> Self {
        let width = ((2.0 * extent) / cell).ceil() as usize + 1;
        Self {
            cell,
            origin: -extent,
            width,
            cells: vec![Vec::new(); width * width],
        }
    }

    fn coord(&self, v: f64) -> usize {
        (((v - self.origin) / self.cell).floor().max(0.0) as usize).min(self.width - 1)
    }

    pub fn insert(&mut self, index: u32, pos: GalaxyPos) {
        let (cx, cy) = (self.coord(pos.x), self.coord(pos.y));
        self.cells[cy * self.width + cx].push(index);
    }

    /// Indices of points whose cell is within `radius` of `pos`. Callers
    /// filter by exact distance.
    pub fn near(&self, pos: GalaxyPos, radius: f64) -> impl Iterator<Item = u32> + '_ {
        let (x0, x1) = (self.coord(pos.x - radius), self.coord(pos.x + radius));
        let (y0, y1) = (self.coord(pos.y - radius), self.coord(pos.y + radius));
        (y0..=y1).flat_map(move |cy| {
            (x0..=x1).flat_map(move |cx| self.cells[cy * self.width + cx].iter().copied())
        })
    }
}
