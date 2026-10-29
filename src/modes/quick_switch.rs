//! Mode selection with the same label input and ordinary bindings as other modes.
use crate::api::hint::{LabelDirection, LabelMatch, LabelPlan, match_label_input};
use crate::api::presentation::{QuickSwitchRow, QuickSwitchView, View};
use crate::api::style::{QuickSwitchPosition, QuickSwitchStyles};
use crate::api::{
    Appearance, Binding, Color, Command, CommandBatch, HostContext, Key, KeyState, Mode, ModeEvent,
    ModeId, Palette, Rect,
};

pub struct Settings {
    pub alphabet: Vec<char>,
    pub blacklist: Vec<String>,
    pub position: QuickSwitchPosition,
    pub light: QuickSwitchStyles,
    pub dark: QuickSwitchStyles,
}

pub struct QuickSwitchMode {
    settings: Settings,
    previous: ModeId,
    active: bool,
    rows: Vec<QuickSwitchRow>,
    prefix: String,
    window: Option<Rect>,
    geometry_id: u64,
}

impl QuickSwitchMode {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            previous: ModeId::normal(),
            active: false,
            rows: Vec::new(),
            prefix: String::new(),
            window: None,
            geometry_id: 0,
        }
    }

    fn render(&self, ctx: &HostContext<'_>) -> Command {
        ctx.present(View::QuickSwitch(QuickSwitchView {
            rows: &self.rows,
            prefix: &self.prefix,
            styles: match ctx.palette.appearance {
                Appearance::Light => &self.settings.light,
                Appearance::Dark => &self.settings.dark,
            },
            position: self.settings.position,
            window: self.window,
        }))
    }

    fn enter(&mut self) -> CommandBatch {
        self.active = true;
        self.prefix.clear();
        self.window = None;
        self.geometry_id = self.geometry_id.wrapping_add(1);
        Command::RequestModeList.into()
    }

    fn set_candidates(
        &mut self,
        mut candidates: Vec<ModeId>,
        ctx: &HostContext<'_>,
    ) -> CommandBatch {
        candidates.retain(|id| {
            *id != self.id()
                && *id != self.previous
                && !id.as_str().starts_with("plugin:")
                && !self
                    .settings
                    .blacklist
                    .iter()
                    .any(|blocked| blocked == id.as_str())
        });
        self.rows.clear();
        if let Some(plan) = LabelPlan::for_stream(
            candidates.len(),
            self.settings.alphabet.len(),
            LabelDirection::Normal,
        ) {
            self.rows
                .extend(
                    candidates
                        .into_iter()
                        .enumerate()
                        .map(|(index, id)| QuickSwitchRow {
                            label: plan.code(index, &self.settings.alphabet).as_str().into(),
                            mode: id,
                        }),
                );
        }
        let mut commands: CommandBatch = self.render(ctx).into();
        if self.settings.position == QuickSwitchPosition::Window {
            commands.push(Command::RequestPanelWindowBounds(self.geometry_id));
        }
        commands
    }
}

impl Mode for QuickSwitchMode {
    fn id(&self) -> ModeId {
        ModeId::quick_switch()
    }

    fn display_name(&self) -> String {
        "Quick Switch".into()
    }

    fn indicator_color(&self, palette: &Palette) -> Option<Color> {
        Some(palette.accent)
    }

    fn claims_key(&self, key: &Key) -> bool {
        key.as_char()
            .is_some_and(|ch| self.settings.alphabet.contains(&ch))
    }

    fn handle_owned(&mut self, event: ModeEvent, ctx: &HostContext<'_>) -> CommandBatch {
        match event {
            ModeEvent::ModeList(modes) if self.active => self.set_candidates(modes, ctx),
            _ => self.handle(&event, ctx),
        }
    }

    fn handle(&mut self, event: &ModeEvent, ctx: &HostContext<'_>) -> CommandBatch {
        match event {
            ModeEvent::ModeList(modes) if self.active => self.set_candidates(modes.clone(), ctx),
            ModeEvent::Activated { previous } => {
                self.previous = previous.clone().unwrap_or_else(ModeId::normal);
                self.enter()
            }
            ModeEvent::Restarted => self.enter(),
            ModeEvent::Deactivated => {
                self.active = false;
                // The next entry gets a fresh registry snapshot.
                self.rows = Vec::new();
                self.prefix.clear();
                self.window = None;
                Command::HideOverlay.into()
            }
            ModeEvent::Binding {
                binding,
                state: KeyState::Down,
                ..
            } => match binding.as_ref() {
                Binding::SelectionCancel => Command::SwitchMode(self.previous.clone()).into(),
                Binding::SelectionParent if self.prefix.pop().is_some() => self.render(ctx).into(),
                _ => CommandBatch::new(),
            },
            ModeEvent::Key {
                key,
                state: KeyState::Down,
                repeat: false,
            } if self.claims_key(key) => {
                let Some(character) = key.as_char() else {
                    return CommandBatch::new();
                };
                self.prefix.push(character);
                match match_label_input(
                    self.rows.iter().map(|row| (row.label.as_str(), &row.mode)),
                    &self.prefix,
                ) {
                    LabelMatch::Complete(mode) => Command::SwitchMode(mode.clone()).into(),
                    LabelMatch::Partial => self.render(ctx).into(),
                    LabelMatch::None => {
                        self.prefix.pop();
                        CommandBatch::new()
                    }
                }
            }
            ModeEvent::PanelWindowBounds { id, bounds }
                if self.active && *id == self.geometry_id =>
            {
                if self.window == *bounds {
                    return CommandBatch::new();
                }
                self.window = *bounds;
                self.render(ctx).into()
            }
            ModeEvent::FocusChanged(_) if self.active => {
                self.window = None;
                self.geometry_id = self.geometry_id.wrapping_add(1);
                let mut commands: CommandBatch = self.render(ctx).into();
                if self.settings.position == QuickSwitchPosition::Window {
                    commands.push(Command::RequestPanelWindowBounds(self.geometry_id));
                }
                commands
            }
            ModeEvent::Resumed | ModeEvent::ScreensChanged(_) | ModeEvent::PointerMoved(_)
                if self.active =>
            {
                self.render(ctx).into()
            }
            _ => CommandBatch::new(),
        }
    }
}
