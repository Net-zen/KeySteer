//! OCR text selection on the same scan, search, multi-selection and input model as UIHint.
mod document;
#[cfg(test)]
mod tests;
use super::hint::{Settings as HintSettings, search, session::ScanSession};
use crate::api::hint as labeling;
use crate::api::{
    Binding, Command, CommandBatch, HostContext, Key, KeyChord, KeyState, Mode, ModeEvent, ModeId,
    Rect, UiScanRequest, UiScanStrategy, UiTarget,
    command::{UiScanResult, UiScanStatus},
    presentation::{
        HintContent, HintSceneCache, HintStyle, HintView, TextChip, TextSelectView, View,
        VisualLayerPlan,
    },
    text_edit::{EditAction, Selection},
    text_select::{Action, Request, Segmented},
    window_presets::TextPrompt,
};
use document::{Frame, Level};
use std::{sync::Arc, time::Duration};
use unicode_segmentation::UnicodeSegmentation;

pub struct Settings {
    pub hint: HintSettings,
    pub alphabet: String,
    pub bindings: Vec<(KeyChord, Arc<Binding>)>,
}

pub struct TextSelectMode {
    config: Settings,
    alphabet: Vec<char>,
    active: bool,
    scan_id: u64,
    segment_id: u64,
    pending_segment: bool,
    frozen: bool,
    source: ScanSession,
    search: ScanSession,
    frame: Frame,
    history: Vec<Frame>,
    base_selected: Vec<bool>,
    page: usize,
    output: String,
    summary: String,
    status: String,
    window: Option<Rect>,
    bounds: Option<Rect>,
    return_mode: ModeId,
    copying: bool,
    layers: VisualLayerPlan,
    scene_cache: HintSceneCache,
}

const PAGE_SIZE: usize = 24;
const MAINTENANCE: &str = "text-select-maintenance";

impl TextSelectMode {
    pub fn new(config: Settings) -> Self {
        let alphabet = config
            .alphabet
            .chars()
            .filter(|c| {
                !config.bindings.iter().any(|(chord, binding)| {
                    !matches!(binding.as_ref(), Binding::Disabled)
                        && chord.keys().len() == 1
                        && chord.keys()[0].as_char() == Some(*c)
                })
            })
            .collect();
        Self {
            config,
            alphabet,
            active: false,
            scan_id: 0,
            segment_id: 0,
            pending_segment: false,
            frozen: false,
            source: ScanSession::default(),
            search: ScanSession::default(),
            frame: Frame::default(),
            history: Vec::new(),
            base_selected: Vec::new(),
            page: 0,
            output: String::new(),
            summary: String::new(),
            status: String::new(),
            window: None,
            bounds: None,
            return_mode: ModeId::normal(),
            copying: false,
            layers: VisualLayerPlan::default(),
            scene_cache: HintSceneCache::default(),
        }
    }

    fn reindex(&mut self) {
        self.scene_cache.invalidate();
        self.search.clear_results();
        self.search.search_text_only = true;
        self.search.search_selection = Selection::default();
        self.search.scanned = self
            .frame
            .units
            .iter()
            .map(|unit| {
                UiTarget::recognized_text(unit.bounds, self.frame.text[unit.range.clone()].into())
            })
            .collect();
        let _ = labeling::assign_compact_into(
            &mut self.search.search_hints,
            self.frame
                .units
                .iter()
                .enumerate()
                .map(|(i, unit)| (unit.bounds, i)),
            &self.alphabet,
            self.config.hint.label_direction,
        );
        self.base_selected.clone_from(&self.frame.selected);
        self.page = 0;
        self.filter(0);
    }

    fn filter(&mut self, budget: usize) {
        // Transfer the existing query buffer while borrowing the shared session.
        let query = std::mem::take(&mut self.search.search_query);
        if search::filter_with_budget(
            &mut self.search,
            &query,
            &self.config.hint.search_match_priority,
            budget,
        ) {
            self.scene_cache.invalidate();
        }
        if query.trim().is_empty() {
            self.frame.selected.clone_from(&self.base_selected);
        } else {
            self.frame.selected.fill(false);
            for hint in &self.search.search_selected {
                self.frame.selected[hint.value] = true;
            }
            // Unresolved text/initials preview the same highest-ranked candidate
            // as UIHint; resolved space-separated terms form the copy selection.
            if self.search.search_selected.is_empty()
                && let Some(position) = self
                    .search
                    .search_focus
                    .as_ref()
                    .map(|f| f.index)
                    .or_else(|| self.search.search_cycle.first())
                && let Some(&index) = self.search.search_matches.get(position)
            {
                self.frame.selected[self.search.search_hints[index].value] = true;
            }
        }
        // An early unresolved query must still see later OCR batches. Freeze
        // the source only once an item actually resolves, or on refinement.
        self.frozen |= !self.search.search_selected.is_empty();
        self.search.search_query = query;
        self.output = self.frame.selected_text();
        let selected = self.frame.selected.iter().filter(|&&s| s).count();
        self.summary = if selected == 0 {
            String::new()
        } else {
            let unit = match self.frame.level {
                Level::Regions => "regions",
                Level::Words => "words",
                Level::Characters => "characters",
            };
            format!(
                "{selected} {unit} · {} chars",
                self.output.graphemes(true).count()
            )
        };
        self.page = self
            .page
            .min(self.search.hints.len().saturating_sub(1) / PAGE_SIZE);
    }

    fn timer() -> Command {
        Command::SetTimer {
            id: MAINTENANCE.into(),
            delay: Duration::ZERO,
            repeating: false,
        }
    }

    fn finish(&self, ctx: &HostContext<'_>, mut commands: CommandBatch) -> CommandBatch {
        commands.push(self.render(ctx));
        // Submit the input/first hints before building pinyin or later batches.
        if !self.pending_segment
            && ((!self.source.scanned.is_empty() && !self.search.search_names_initialized)
                || (!self.frozen && self.source.pending_relabel))
        {
            commands.push(Self::timer());
        }
        commands
    }

    fn rebuild_source(&mut self) {
        let query = std::mem::take(&mut self.search.search_query);
        let edit = self.search.search_selection;
        self.frame = Frame::from_targets(&self.source.scanned);
        self.reindex();
        self.search.search_query = query;
        self.search.search_selection = edit;
        self.filter(0);
    }

    fn maintain(&mut self, ctx: &HostContext<'_>) -> CommandBatch {
        let changed = !self.frozen && self.source.pending_relabel;
        if changed {
            self.source.apply_deferred();
            self.source.pending_relabel = false;
            self.rebuild_source();
        }
        let pending = self.search.search_pending;
        let ready = self
            .search
            .prepare_search_names(if pending { 1024 } else { 256 });
        if ready && pending {
            self.filter(0);
        }
        if changed || pending && ready {
            return self.finish(ctx, self.selection_pointer());
        }
        if !ready {
            CommandBatch::one(Self::timer())
        } else {
            CommandBatch::new()
        }
    }

    fn start(&mut self, ctx: &HostContext<'_>) -> CommandBatch {
        self.active = true;
        self.scan_id = self.scan_id.wrapping_add(1);
        self.segment_id = self.segment_id.wrapping_add(1);
        self.pending_segment = false;
        self.copying = false;
        self.frozen = false;
        self.source.clear_results();
        self.scene_cache.clear();
        self.frame = Frame::default();
        self.history.clear();
        self.window = None;
        self.status = "Recognizing text…".into();
        self.reindex();
        let screens = if self.config.hint.scan_scope == crate::api::UiScanScope::All {
            ctx.screens
        } else {
            ctx.active_screen()
                .map(std::slice::from_ref)
                .unwrap_or_default()
        };
        let bounds = crate::api::Screen::virtual_bounds(screens);
        self.bounds = Some(bounds);
        self.source.scan_bounds = self.bounds;
        let mut vision = self.config.hint.vision.clone();
        vision.detect_text = true;
        vision.detect_rectangles = false;
        vision.text_positions = true;
        let mut commands = CommandBatch::two(
            Command::CancelTimer {
                id: MAINTENANCE.into(),
            },
            Command::CancelTextSegmentation,
        );
        commands.push(Command::scan_ui(UiScanRequest {
            id: self.scan_id,
            scope: self.config.hint.scan_scope,
            screen_bounds: if screens.len() > 1 {
                screens.iter().map(|s| s.bounds).collect()
            } else {
                Vec::new()
            },
            timeout_ms: self.config.hint.scan_timeout_ms,
            bounds: Some(bounds),
            roles: Vec::new(),
            max_depth: self.config.hint.max_depth,
            visible_only: true,
            clickable_only: false,
            strategy: UiScanStrategy::Vision,
            vision,
            app: ctx.focused_app.cloned(),
        }));
        let (bounds, style) =
            ctx.presenter
                .input_panel(&self.config.hint.search_input_ui, self.window, ctx);
        commands.push(Command::OpenTextPrompt(Box::new(TextPrompt {
            point_keys: None,
            edit_keys: self.config.hint.search_edit_keys.clone(),
            live_style: Some(style),
            copy_keys: Arc::from([]),
            bounds,
            id: self.scan_id,
            title: String::new(),
            message: String::new(),
            placeholder: String::new(),
            max_chars: 4096,
        })));
        commands.push(Command::RequestPanelWindowBounds(self.scan_id));
        self.finish(ctx, commands)
    }

    fn scan(&mut self, mut result: UiScanResult, ctx: &HostContext<'_>) -> CommandBatch {
        if !self.active || result.id != self.scan_id {
            return CommandBatch::new();
        }
        result
            .targets
            .retain(|t| !t.name.trim().is_empty() || !t.ocr_text().trim().is_empty());
        self.status = match result.status {
            UiScanStatus::Partial => "Recognizing text…",
            UiScanStatus::PermissionDenied(_) => "Screen recording permission required",
            UiScanStatus::Unsupported(_) => "System OCR unavailable",
            UiScanStatus::ContextChanged => "Screen changed — reopen Text Select",
            UiScanStatus::Failed(_) => "Text recognition failed",
            UiScanStatus::TimedOut => "Text recognition timed out",
            _ => "No text found",
        }
        .into();
        if self.frozen || !self.source.scanned.is_empty() {
            self.source.defer_update(result.targets, result.retired);
            return if self.frozen {
                CommandBatch::new()
            } else {
                CommandBatch::one(Self::timer())
            };
        }
        self.source.apply_update(result.targets, &result.retired);
        self.rebuild_source();
        self.finish(ctx, CommandBatch::new())
    }

    fn refine(&mut self, ctx: &HostContext<'_>) -> CommandBatch {
        if self.pending_segment || self.frame.level == Level::Characters {
            return CommandBatch::new();
        }
        // Explicit acceptance may finish the remaining index, as in UIHint.
        self.filter(usize::MAX);
        if self.output.is_empty() {
            return self.finish(ctx, CommandBatch::new());
        }
        let level = if self.frame.level == Level::Regions {
            Level::Words
        } else {
            Level::Characters
        };
        let next = self.frame.descend(level);
        self.history.push(std::mem::replace(&mut self.frame, next));
        self.frozen = true;
        self.reindex();
        let mut commands = CommandBatch::new();
        if level == Level::Words {
            self.segment_id = self.segment_id.wrapping_add(1);
            self.pending_segment = true;
            self.status = "Segmenting words…".into();
            commands.push(Command::SegmentText(Request {
                id: self.segment_id,
                text: self.frame.text.clone(),
            }));
        } else {
            self.status.clear();
        }
        self.finish(ctx, commands)
    }

    fn segmented(&mut self, result: Segmented, ctx: &HostContext<'_>) -> CommandBatch {
        if !self.active || !self.pending_segment || result.id != self.segment_id {
            return CommandBatch::new();
        }
        self.pending_segment = false;
        self.frame.apply_words(&result.words);
        let query = std::mem::take(&mut self.search.search_query);
        let edit = self.search.search_selection;
        self.reindex();
        self.search.search_query = query;
        self.search.search_selection = edit;
        self.filter(0);
        self.status.clear();
        self.finish(ctx, CommandBatch::new())
    }

    fn action(&mut self, action: Action, ctx: &HostContext<'_>) -> CommandBatch {
        match action {
            Action::NextPage => {
                self.page =
                    (self.page + 1).min(self.search.hints.len().saturating_sub(1) / PAGE_SIZE)
            }
            Action::PreviousPage => self.page = self.page.saturating_sub(1),
            Action::Refine => return self.refine(ctx),
            Action::Copy => {
                self.filter(usize::MAX);
                if !self.output.is_empty() && !self.pending_segment {
                    self.copying = true;
                    return CommandBatch::one(Command::CopyText(self.output.clone()));
                }
            }
            Action::Search => {
                // Search is always open. The configurable action selects its
                // query for replacement instead of introducing another mode.
                self.search
                    .search_selection
                    .edit(&mut self.search.search_query, EditAction::SelectAll);
            }
        }
        self.finish(ctx, CommandBatch::new())
    }

    fn navigate(&mut self, reset: bool, ctx: &HostContext<'_>) -> CommandBatch {
        self.segment_id = self.segment_id.wrapping_add(1);
        self.pending_segment = false;
        self.copying = false;
        if reset || self.history.is_empty() {
            self.history.clear();
            self.frozen = false;
            self.source.apply_deferred();
            self.source.pending_relabel = false;
            self.frame = Frame::from_targets(&self.source.scanned);
        } else if let Some(frame) = self.history.pop() {
            self.frame = frame;
        }
        self.reindex();
        self.status.clear();
        self.finish(ctx, CommandBatch::one(Command::CancelTextSegmentation))
    }

    fn query_changed(&mut self, ctx: &HostContext<'_>) -> CommandBatch {
        self.copying = false;
        self.page = 0;
        self.search.search_focus = None;
        if !self.pending_segment {
            self.filter(0);
        }
        self.finish(ctx, self.selection_pointer())
    }

    fn selection_pointer(&self) -> CommandBatch {
        let mut commands = CommandBatch::new();
        if self.frame.level == Level::Regions
            && let Some((unit, _)) = self
                .frame
                .units
                .iter()
                .zip(&self.frame.selected)
                .rfind(|(_, selected)| **selected)
        {
            let rect = unit.bounds;
            commands.push(Command::WarpPointer {
                x: rect.right() - 1.0,
                y: rect.bottom() - 1.0,
            });
        }
        commands
    }

    fn insert(&mut self, value: &str, ctx: &HostContext<'_>) -> CommandBatch {
        let remaining = 4096usize.saturating_sub(
            self.search.search_query.chars().count()
                - self.search.search_query[self.search.search_selection.range()]
                    .chars()
                    .count(),
        );
        let end = value
            .char_indices()
            .nth(remaining)
            .map_or(value.len(), |(i, _)| i);
        self.search
            .search_selection
            .insert(&mut self.search.search_query, &value[..end]);
        self.query_changed(ctx)
    }

    fn render(&self, ctx: &HostContext<'_>) -> Command {
        let style = HintStyle {
            ui: &self.config.hint.ui,
            placement: self.config.hint.placement,
            label_x_offset: self.config.hint.label_x_offset,
            label_y_offset: self.config.hint.label_y_offset,
            boundary_highlight: &self.config.hint.boundary_highlight,
            search_input_ui: &self.config.hint.search_input_ui,
            search_highlight: &self.config.hint.search_highlight,
        };
        let hints = HintView {
            cache: Some(&self.scene_cache),
            point: None,
            info: None,
            window_bounds: &self.window,
            content: HintContent {
                hints: if self.frame.level != Level::Characters && !self.pending_segment {
                    &self.search.hints
                } else {
                    &[]
                },
                uniform_label_chars: None,
                prefix: "",
                search: Some(&self.search.search_query),
                search_selection: self.search.search_selection,
                scan_bounds: self.bounds,
                style,
            },
            layers: &self.layers,
            active_layer: None,
        };
        let chips: Vec<_> = if self.frame.level == Level::Characters {
            self.search
                .hints
                .iter()
                .skip(self.page * PAGE_SIZE)
                .take(PAGE_SIZE)
                .map(|hint| TextChip {
                    code: hint.label.as_str(),
                    text: &self.frame.text[self.frame.units[hint.value].range.clone()],
                    selected: self.frame.selected[hint.value],
                })
                .collect()
        } else {
            Vec::new()
        };
        let status = if self.pending_segment {
            "Segmenting words…"
        } else if self.search.search_pending {
            "Searching…"
        } else if self.frame.units.is_empty() {
            &self.status
        } else if !self.search.search_query.trim().is_empty() && self.output.is_empty() {
            "No matching text"
        } else {
            "Select text to preview"
        };
        Command::ShowOverlay(Arc::new(
            ctx.presenter.compose(
                View::TextSelect(TextSelectView {
                    hints,
                    ui: &self.config.hint.search_info_ui,
                    chips: &chips,
                    text: &self.output,
                    status,
                    summary: &self.summary,
                    page: (
                        self.page + 1,
                        self.search.hints.len().div_ceil(PAGE_SIZE).max(1),
                    ),
                    selected_bounds: self
                        .frame
                        .units
                        .iter()
                        .zip(&self.frame.selected)
                        .filter(|(_, selected)| **selected)
                        .map(|(u, _)| u.bounds)
                        .collect(),
                }),
                ctx,
            ),
        ))
    }
}

impl Mode for TextSelectMode {
    fn id(&self) -> ModeId {
        ModeId::text_select()
    }
    fn display_name(&self) -> String {
        "Text Select".into()
    }
    fn indicator_detail(&self) -> Option<String> {
        Some(self.frame.level.title().into())
    }
    fn keyboard_prompt_supported(&self) -> bool {
        true
    }
    fn keyboard_prompt_active(&self) -> bool {
        self.active
    }
    fn keyboard_prompt_binding(&self, key: &Key, pressed: &[Key]) -> Option<Arc<Binding>> {
        self.config
            .bindings
            .iter()
            .find(|(chord, binding)| {
                if matches!(binding.as_ref(), Binding::Disabled)
                    && chord.keys().len() == 1
                    && chord.keys()[0].as_char().is_some()
                {
                    return false;
                }
                if key.as_str() == "backspace"
                    && chord.keys().len() == 1
                    && !self.search.search_query.is_empty()
                    && matches!(binding.as_ref(), Binding::SelectionParent)
                {
                    return false;
                }
                chord.activation_matches(key)
                    && chord.keys().len() == pressed.len()
                    && chord.matches_pressed(pressed)
            })
            .map(|(_, binding)| binding.clone())
    }
    fn wants_pointer_events(&self) -> bool {
        false
    }
    fn claims_key(&self, key: &Key) -> bool {
        key.as_char().is_some()
    }
    fn handle_owned(&mut self, event: ModeEvent, ctx: &HostContext<'_>) -> CommandBatch {
        match event {
            ModeEvent::UiScanned(result) => self.scan(result, ctx),
            ModeEvent::TextSegmented(result) => self.segmented(result, ctx),
            event => self.handle(&event, ctx),
        }
    }
    fn handle(&mut self, event: &ModeEvent, ctx: &HostContext<'_>) -> CommandBatch {
        match event {
            ModeEvent::Activated { previous } => {
                self.return_mode = previous
                    .clone()
                    .filter(|id| *id != self.id())
                    .unwrap_or_else(ModeId::idle);
                self.start(ctx)
            }
            ModeEvent::Restarted | ModeEvent::ScreensChanged(_) => self.start(ctx),
            ModeEvent::Deactivated => {
                self.active = false;
                self.source.clear_results();
                self.scene_cache.clear();
                self.search.clear_results();
                self.history.clear();
                self.frame = Frame::default();
                self.output.clear();
                CommandBatch::from_iter([
                    Command::CancelTimer {
                        id: MAINTENANCE.into(),
                    },
                    Command::CancelTextSegmentation,
                    Command::ReleaseTextPrompt,
                    Command::HideOverlay,
                ])
            }
            ModeEvent::Resumed => self.finish(ctx, CommandBatch::new()),
            ModeEvent::Timer { id, .. } if self.active && id == MAINTENANCE => self.maintain(ctx),
            ModeEvent::UiScanned(result) => self.scan(result.clone(), ctx),
            ModeEvent::TextSegmented(result) => self.segmented(result.clone(), ctx),
            ModeEvent::PanelWindowBounds { id, bounds } if *id == self.scan_id => {
                self.window = *bounds;
                self.finish(ctx, CommandBatch::new())
            }
            ModeEvent::Binding {
                binding,
                state: KeyState::Down,
                ..
            } => match binding.as_ref() {
                Binding::TextSelect(action) => self.action(*action, ctx),
                Binding::SelectionReset => self.navigate(true, ctx),
                Binding::SelectionParent => self.navigate(false, ctx),
                Binding::SelectionAccept => self.action(Action::Copy, ctx),
                Binding::SelectionSearch => self.action(Action::Search, ctx),
                Binding::SelectionCancel | Binding::Escape => {
                    CommandBatch::one(Command::SwitchMode(self.return_mode.clone()))
                }
                Binding::RescanUi => self.start(ctx),
                _ => CommandBatch::one(Command::DispatchActions(vec![(**binding).clone()])),
            },
            ModeEvent::TextInserted(c) => self.insert(c.encode_utf8(&mut [0; 4]), ctx),
            ModeEvent::TextPasted(text) => self.insert(text, ctx),
            ModeEvent::TextCopied if self.copying => {
                self.copying = false;
                CommandBatch::one(Command::SwitchMode(self.return_mode.clone()))
            }
            ModeEvent::TextSubmitted(None) | ModeEvent::TextEdit(EditAction::Cancel) => {
                CommandBatch::one(Command::SwitchMode(self.return_mode.clone()))
            }
            ModeEvent::TextEdit(EditAction::Accept) => self.action(Action::Copy, ctx),
            ModeEvent::TextEdit(EditAction::Paste) => CommandBatch::one(Command::ReadClipboard),
            ModeEvent::TextEdit(action) => {
                if matches!(action, EditAction::Copy | EditAction::Cut) {
                    return CommandBatch::one(Command::CopyInputText {
                        text: self.search.search_query[self.search.search_selection.range()].into(),
                        cut: *action == EditAction::Cut,
                    });
                }
                let changed = self
                    .search
                    .search_selection
                    .edit(&mut self.search.search_query, *action);
                if changed {
                    self.query_changed(ctx)
                } else {
                    self.finish(ctx, CommandBatch::new())
                }
            }
            _ => CommandBatch::new(),
        }
    }
}
