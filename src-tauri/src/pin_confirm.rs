//! The PIN-confirmation prompt that is waiting on the user, if any. Pure, so the rule that keeps
//! one pairing's leftovers out of the next is unit-tested.
//!
//! Windows asks to confirm a PIN on a pool thread; tug shows the dialog, and a helper thread waits
//! (up to a minute) for the answer, then completes Windows' deferral and closes the dialog. That
//! helper used to clear whatever prompt was showing when it finished, so one left over from a
//! cancelled or timed-out pairing closed the *next* pairing's dialog and threw its answer away.
//! Now every prompt has an id, and only the prompt with that id can be closed by its helper.

/// `T` is the channel the user's answer goes down (a `SyncSender<bool>` in the app).
#[derive(Debug)]
pub struct PinConfirm<T> {
    /// The prompt on screen, by id.
    open: Option<u64>,
    /// Where its answer goes, until the user gives one.
    answer: Option<T>,
    last_id: u64,
}

impl<T> Default for PinConfirm<T> {
    fn default() -> Self {
        Self {
            open: None,
            answer: None,
            last_id: 0,
        }
    }
}

impl<T> PinConfirm<T> {
    /// A new prompt replaces any other: returns its id.
    pub fn open(&mut self, answer: T) -> u64 {
        self.last_id = self.last_id.wrapping_add(1).max(1);
        self.open = Some(self.last_id);
        self.answer = Some(answer);
        self.last_id
    }

    /// The user answered: take the channel to send it down (once). The prompt stays open until its
    /// helper has acted on the answer and closes it.
    pub fn take(&mut self) -> Option<T> {
        self.answer.take()
    }

    /// Close prompt `id`, dropping its channel (which wakes its helper at once). Returns whether it
    /// was the prompt open, so only then is the dialog told to close: a later prompt is left alone.
    pub fn close(&mut self, id: u64) -> bool {
        if self.open != Some(id) {
            return false;
        }
        self.open = None;
        self.answer = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leftover_helper_cannot_close_the_next_pairings_prompt() {
        let mut slot = PinConfirm::default();
        let first = slot.open("first");
        // The first pairing was cancelled; a new one asks before the first's helper times out.
        let second = slot.open("second");
        assert_ne!(first, second);
        assert!(
            !slot.close(first),
            "the leftover helper finds a newer prompt and leaves it"
        );
        assert_eq!(
            slot.take(),
            Some("second"),
            "the new prompt's answer still goes through"
        );
        assert!(slot.close(second));
        assert!(!slot.close(second), "closed once");
    }

    #[test]
    fn the_answer_is_taken_once_and_closing_drops_an_unanswered_channel() {
        let mut slot = PinConfirm::default();
        let id = slot.open(7);
        assert_eq!(slot.take(), Some(7));
        assert_eq!(slot.take(), None);
        assert!(slot.close(id), "answered prompts are closed by their helper");
        let id = slot.open(8);
        assert!(slot.close(id));
        assert_eq!(slot.take(), None, "a closed prompt can't be answered");
    }

    #[test]
    fn closing_nothing_or_a_stale_id_does_nothing() {
        let mut slot: PinConfirm<u8> = PinConfirm::default();
        assert!(!slot.close(1));
        assert!(!slot.close(0));
    }
}
