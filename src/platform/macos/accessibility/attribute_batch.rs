//! Bounded per-node AX reads; kept independent of CF so cancellation and
//! fallback ownership can be checked on every host.

pub(super) enum Batch<T, const N: usize> {
    Values([Option<T>; N]),
    Unsupported,
    Failed,
}

#[derive(Default)]
pub(super) struct Reader {
    individual_only: bool,
}

impl Reader {
    #[cfg(test)]
    pub(super) fn individual() -> Self {
        Self {
            individual_only: true,
        }
    }

    /// Capability state lasts for one window, never retaining node values.
    pub(super) fn reset(&mut self) {
        self.individual_only = false;
    }

    pub(super) fn read<T, const N: usize>(
        &mut self,
        count: usize,
        current: impl Fn() -> bool,
        batch: impl FnOnce() -> Batch<T, N>,
        mut single: impl FnMut(usize) -> Option<T>,
    ) -> Option<[Option<T>; N]> {
        assert!(count <= N);
        if !current() {
            return None;
        }
        if !self.individual_only {
            let result = batch();
            if !current() {
                return None;
            }
            match result {
                Batch::Values(values) => return Some(values),
                Batch::Unsupported => self.individual_only = true,
                // A dead or unresponsive provider must not incur N more calls.
                Batch::Failed => return Some(std::array::from_fn(|_| None)),
            }
        }
        let mut values = std::array::from_fn(|_| None);
        for (index, slot) in values.iter_mut().enumerate().take(count) {
            if !current() {
                return None;
            }
            *slot = single(index);
        }
        current().then_some(values)
    }
}

pub(super) fn unsupported(error: i32) -> bool {
    // AXErrorIllegalArgument, AXErrorAttributeUnsupported, AXErrorNotImplemented.
    // The native adapter supplies a valid, fixed list; these permit the old
    // single-attribute API as a compatibility fallback. CannotComplete,
    // InvalidUIElement and other failures retain their ordinary missing values.
    matches!(error, -25201 | -25205 | -25208)
}
