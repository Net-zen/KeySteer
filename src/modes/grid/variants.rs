//! Each geometry has one concrete style, view and identity.

use crate::api::presentation::{
    BisectStyle, BisectView, GridLayout, GridStyle, GridView, RecursiveGridStyle,
    RecursiveGridView, View,
};
use crate::api::{Color, ModeId, Palette};
use crate::modes::targeting::{
    BisectGeometry, Geometry, GridGeometry, LetterGeometry, RecursiveGeometry, TargetingSession,
};

pub(crate) trait GridPresentation: Geometry {
    type Style: Send;
    const NAME: &'static str;
    fn id() -> ModeId;
    fn indicator_color(palette: &Palette) -> Color;
    fn view<'a>(
        &'a self,
        session: &TargetingSession,
        max_depth: u32,
        style: &'a Self::Style,
    ) -> View<'a>;
}

fn layout_at(geometry: &impl LetterGeometry, depth: u32) -> GridLayout<'_> {
    let layout = geometry.layout_at(depth);
    GridLayout {
        rows: layout.rows,
        cols: layout.cols,
        keys: &layout.keys,
    }
}

impl GridPresentation for GridGeometry {
    type Style = GridStyle;
    const NAME: &'static str = "Grid";
    fn id() -> ModeId {
        ModeId::grid()
    }
    fn indicator_color(palette: &Palette) -> Color {
        palette.accent
    }
    fn view<'a>(
        &'a self,
        session: &TargetingSession,
        max_depth: u32,
        ui: &'a GridStyle,
    ) -> View<'a> {
        View::Grid(GridView {
            layout: layout_at(self, session.depth()),
            ui,
            current: session.current(),
            root: session.root(),
            terminal: session.terminal,
            depth: session.depth(),
            max_depth,
        })
    }
}

impl GridPresentation for RecursiveGeometry {
    type Style = RecursiveGridStyle;
    const NAME: &'static str = "Recursive Grid";
    fn id() -> ModeId {
        ModeId::recursive_grid()
    }
    fn indicator_color(palette: &Palette) -> Color {
        palette.accent_alt
    }
    fn view<'a>(
        &'a self,
        session: &TargetingSession,
        max_depth: u32,
        ui: &'a RecursiveGridStyle,
    ) -> View<'a> {
        View::RecursiveGrid(RecursiveGridView {
            layout: layout_at(self, session.depth()),
            next_layout: layout_at(self, session.depth() + 1),
            ui,
            current: session.current(),
            root: session.root(),
            terminal: session.terminal,
            can_descend: self.can_descend(session, max_depth),
        })
    }
}

impl GridPresentation for BisectGeometry {
    type Style = BisectStyle;
    const NAME: &'static str = "Bisect";
    fn id() -> ModeId {
        ModeId::bisect()
    }
    fn indicator_color(palette: &Palette) -> Color {
        palette.accent_alt
    }
    fn view<'a>(
        &'a self,
        session: &TargetingSession,
        max_depth: u32,
        ui: &'a BisectStyle,
    ) -> View<'a> {
        View::Bisect(BisectView {
            ui,
            current: session.current(),
            root: session.root(),
            terminal: session.terminal,
            axes: self.axes(session, max_depth),
        })
    }
}
