//! Read-only window presentation data; no scene construction.
use super::*;
use crate::api::presentation::{View, WindowView};

impl WindowSession {
    pub(super) fn detail(&self) -> String {
        if self.kind == WindowKind::Tab {
            return self.tab_detail();
        }
        if self.library_open {
            let mut detail = format!(
                "{}\nType a layout number · page {} / {}",
                if self.deleting_presets {
                    "Delete layouts"
                } else {
                    "Restore"
                },
                self.library_page + 1,
                self.saved_presets
                    .len()
                    .div_ceil(super::presets::PAGE_SIZE)
                    .max(1)
            );
            if let Some(layout) = &self.delete_selection {
                return format!(
                    "Delete layouts\nDelete {}   {}?\nConfirm deletion or toggle back to Restore to cancel",
                    layout.id,
                    layout.name()
                );
            }
            if self.saved_presets.is_empty() {
                detail.push_str("\nNo saved layouts · save a layout in the editor");
            }
            for layout in self
                .saved_presets
                .iter()
                .skip(self.library_page * super::presets::PAGE_SIZE)
                .take(super::presets::PAGE_SIZE)
            {
                detail.push_str(&format!("\n{}   {}", layout.id, layout.name()));
            }
            if !self.number.display.is_empty() {
                detail.push_str(&format!("\nInput: {}", self.number.display));
            }
            if let Some(status) = &self.status {
                detail.push_str(&format!("\n{status}"));
            }
            return detail;
        }
        if let Some(input) = &self.multi_input {
            let mappings = self
                .settings
                .multi_bindings
                .iter()
                .map(|(chord, binding)| {
                    format!(
                        "{}: {}",
                        crate::api::input::display_key_chord(&chord.canonical()),
                        match binding.as_ref() {
                            Binding::Window(W::MultiConfirm) => "Confirm",
                            Binding::Window(W::ClearMulti) => "Clear",
                            Binding::Send(chord) =>
                                match crate::api::text_edit::navigation_action(chord) {
                                    Some(crate::api::text_edit::EditAction::Left) => "←",
                                    Some(crate::api::text_edit::EditAction::Right) => "→",
                                    Some(crate::api::text_edit::EditAction::Home) => "Home",
                                    Some(crate::api::text_edit::EditAction::End) => "End",
                                    Some(crate::api::text_edit::EditAction::Backspace) =>
                                        "Backspace",
                                    Some(crate::api::text_edit::EditAction::Delete) => "Delete",
                                    _ => "Select",
                                },
                            _ => "",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let (before, after) = input.text.split_at(input.selection.cursor);
            return format!(
                "Multi-select · {} selected\nInput: {before}▏{after}\n{mappings}",
                self.operation_targets().len(),
            );
        }
        let state = match self.edit.as_ref().map(|e| &e.model) {
            Some(EditModel::Quick(quick)) => format!(
                "Quick · {}",
                quick.caption_with_ticks(&self.settings.ratio_ticks)
            ),
            Some(EditModel::Tree(tree)) => format!("Edit · area `{}", tree.selected),
            None => match self.kind {
                WindowKind::Quick => "Quick",
                WindowKind::Editor => "Edit",
                _ if self.size => "Resize",
                _ => "Move",
            }
            .into(),
        };
        let mut detail = state;
        if self.kind == WindowKind::Move && self.multi_anchor.is_some() {
            detail.push_str(&format!(" · {} selected", self.operation_targets().len()));
        }
        if let Some(window) = &self.target {
            let app = window
                .app
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&window.app)
                .trim_end_matches(".exe");
            detail.push_str(&format!("\n{app}\n{}", window.title));
            if window.minimized {
                detail.push_str("\nMinimized · cycle to restore");
            }
        }
        if !self.number.display.is_empty() {
            detail.push_str("\nInput: ");
            detail.push_str(&self.number.display);
            if self.number.pending() {
                detail.push('…');
            }
        }
        if let Some(source) = self.swap_source.and_then(|id| self.numbers.get(&id)) {
            detail.push_str(&format!("\nWindow {source} → window number / `area"));
        }
        if let Some(status) = &self.status {
            detail.push('\n');
            detail.extend(status.chars().take(140));
        }
        detail
    }

    pub(super) fn view(&self) -> View<'_> {
        if self.temporary {
            return View::Empty;
        }
        if self.library_open {
            return View::Empty;
        }
        let tree = self
            .edit
            .as_ref()
            .filter(|edit| edit.ready)
            .and_then(|edit| match &edit.model {
                EditModel::Tree(tree) => Some(tree),
                _ => None,
            });
        View::Window(WindowView {
            selected: if matches!(self.kind, WindowKind::Move | WindowKind::Quick) {
                &self.multi
            } else {
                &[]
            },
            text_cache: Some(&self.text_cache),
            configurable_position: matches!(self.kind, WindowKind::Move | WindowKind::Editor),
            tabs: &self.tabs.state,
            group_input: self.kind == WindowKind::Tab && self.number.slot,
            styles: &self.settings.styles,
            border_width: self.settings.border_width,
            target: self.target.as_ref().filter(|w| {
                !w.minimized
                    && (!matches!(self.kind, WindowKind::Move | WindowKind::Quick)
                        || self.multi_anchor.is_none()
                        || self.multi.iter().any(|id| {
                            self.tabs.state.representative(*id)
                                == self.tabs.state.representative(w.id)
                        }))
            }),
            screen: self.screen,
            inventory: &self.inventory,
            visible: &self.visible,
            numbers: &self.numbers,
            tree,
            gap: self.settings.gap * self.edit.as_ref().map_or(1.0, |edit| edit.gap_scale),
        })
    }
}
