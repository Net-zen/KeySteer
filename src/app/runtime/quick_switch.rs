//! Only the entry gesture is special; the picker is an ordinary registered mode.
use super::*;
use crate::api::input::InputEvent;

#[derive(Default)]
pub(super) struct QuickSwitcher {
    pub(super) pending: Option<Pending>,
    // Suppress the original hold's repeats, even after selecting another mode.
    blocked_trigger: Option<Key>,
}

pub(super) struct Pending {
    key: Key,
    pub(super) deadline: Instant,
}

impl Engine {
    pub(super) fn cancel_quick_switch(&mut self, capture_lost: bool) {
        self.quick_switch.pending = None;
        if capture_lost {
            self.quick_switch.blocked_trigger = None;
        }
    }

    pub(super) fn quick_switch_key(
        &mut self,
        input: &InputEvent,
        backend: &mut dyn Backend,
    ) -> Result<bool, String> {
        if self.quick_switch.pending.is_none()
            && self.quick_switch.blocked_trigger.is_none()
            && input.key != self.settings.quick_switch.key
        {
            return Ok(false);
        }
        if input.injected {
            return Ok(false);
        }
        if input.state == KeyState::Up {
            if self.quick_switch.blocked_trigger.as_ref() == Some(&input.key) {
                self.quick_switch.blocked_trigger = None;
            }
            if self
                .quick_switch
                .pending
                .as_ref()
                .is_some_and(|pending| pending.key == input.key)
            {
                self.quick_switch.pending = None;
            }
            // Always let ordinary routing release the original binding's owner.
            return Ok(false);
        }
        let elapsed =
            self.quick_switch.pending.as_ref().is_some_and(|pending| {
                pending.key == input.key && Instant::now() >= pending.deadline
            });
        if self.quick_switch.blocked_trigger.as_ref() == Some(&input.key) || elapsed {
            self.quick_switch.blocked_trigger = Some(input.key.clone());
            self.dispose_input(input, KeyOutcome::Consumed, false, backend)?;
            return Ok(true);
        }
        if input.repeat {
            return Ok(false);
        }
        // A chord is not a long press; other keys keep their ordinary behavior.
        if self.quick_switch.pending.is_some() {
            self.quick_switch.pending = None;
            return Ok(false);
        }
        if self.settings.quick_switch.enabled
            && self.enabled
            && !self.is_excluded_app()
            && self.registry.active != ModeId::idle()
            && self.registry.active != ModeId::text_input()
            && self.registry.active != ModeId::quick_switch()
            && self.window_presets.pending.is_none()
            && self.scheduler.text_prompt.is_none()
            && input.key == self.settings.quick_switch.key
            && self.input.pressed.is_empty()
        {
            self.quick_switch.pending = Some(Pending {
                key: input.key.clone(),
                deadline: Instant::now()
                    + Duration::from_millis(self.settings.quick_switch.hold_ms),
            });
        }
        Ok(false)
    }

    pub(super) fn fire_quick_switch(&mut self, backend: &mut dyn Backend) -> Result<(), String> {
        if !self.enabled
            || self.is_excluded_app()
            || self.window_presets.pending.is_some()
            || self.scheduler.text_prompt.is_some()
        {
            self.quick_switch.pending = None;
        }
        if self
            .quick_switch
            .pending
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.deadline)
            && let Some(pending) = self.quick_switch.pending.take()
        {
            self.quick_switch.blocked_trigger = Some(pending.key);
            self.execute([Command::SwitchMode(ModeId::quick_switch())], backend)?;
        }
        Ok(())
    }
}
