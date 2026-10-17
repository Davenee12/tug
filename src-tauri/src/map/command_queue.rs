//! The message worker's to-do list, in priority order. Pure, so the ordering is unit-tested.
//!
//! The worker (`service`) does one thing at a time on one Bluetooth link, and some of those things
//! are slow: a refresh (inbox listing, one download per new text, a contacts check, a calls pull),
//! marking texts read one at a time, a reconnect. A text the user just sent used to wait behind
//! all of that. Now every command lands here first and comes out by lane: Start over first, then
//! sends (and the hands-free dial, which also has someone waiting on it), then everything else in
//! arrival order, and marking texts read last, once nothing is waiting to send. The worker also
//! looks here between the phases of a refresh (`take_priority`), so a send queued mid-refresh goes
//! out next rather than after the whole refresh.

use std::collections::VecDeque;

/// How urgent a command is. Lower runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lane {
    /// Start over: everything after it must see the reset.
    Urgent,
    /// Someone is waiting on it right now: a text being sent, a call being placed.
    Send,
    /// Refreshes, checks, the inventory.
    Normal,
    /// Marking texts read on the phone: nobody waits on it, so never ahead of a send.
    Background,
}

const LANES: usize = 4;

impl Lane {
    fn index(self) -> usize {
        self as usize
    }

    /// Runs between the phases of a refresh instead of after it.
    pub fn is_priority(self) -> bool {
        self <= Lane::Send
    }
}

pub trait Laned {
    fn lane(&self) -> Lane;
    /// Whether this is the same request as `other` already waiting, so it needn't be queued twice
    /// (two refreshes in a row are one refresh).
    fn same_as(&self, _other: &Self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct CommandQueue<T> {
    lanes: [VecDeque<T>; LANES],
}

impl<T> Default for CommandQueue<T> {
    fn default() -> Self {
        Self {
            lanes: std::array::from_fn(|_| VecDeque::new()),
        }
    }
}

impl<T: Laned> CommandQueue<T> {
    pub fn push(&mut self, item: T) {
        let lane = &mut self.lanes[item.lane().index()];
        if lane.iter().any(|queued| queued.same_as(&item)) {
            return;
        }
        lane.push_back(item);
    }

    /// The next command: the most urgent lane first, oldest first within a lane.
    pub fn pop(&mut self) -> Option<T> {
        self.lanes.iter_mut().find_map(VecDeque::pop_front)
    }

    /// The next command that shouldn't wait for a refresh in progress (`Lane::is_priority`).
    pub fn take_priority(&mut self) -> Option<T> {
        self.lanes[..=Lane::Send.index()]
            .iter_mut()
            .find_map(VecDeque::pop_front)
    }

    pub fn has_priority(&self) -> bool {
        self.lanes[..=Lane::Send.index()].iter().any(|l| !l.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.iter().all(VecDeque::is_empty)
    }

    pub fn len(&self) -> usize {
        self.lanes.iter().map(VecDeque::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Job {
        Forget,
        Deliver(u32),
        Dial,
        Refresh,
        CheckContacts,
        MarkRead(u32),
    }

    impl Laned for Job {
        fn lane(&self) -> Lane {
            match self {
                Job::Forget => Lane::Urgent,
                Job::Deliver(_) | Job::Dial => Lane::Send,
                Job::Refresh | Job::CheckContacts => Lane::Normal,
                Job::MarkRead(_) => Lane::Background,
            }
        }

        fn same_as(&self, other: &Self) -> bool {
            matches!((self, other), (Job::Refresh, Job::Refresh))
        }
    }

    fn drain(q: &mut CommandQueue<Job>) -> Vec<Job> {
        std::iter::from_fn(|| q.pop()).collect()
    }

    #[test]
    fn a_send_goes_before_reads_and_refreshes_queued_ahead_of_it() {
        let mut q = CommandQueue::default();
        q.push(Job::MarkRead(1));
        q.push(Job::Refresh);
        q.push(Job::CheckContacts);
        q.push(Job::Deliver(7));
        q.push(Job::MarkRead(2));
        q.push(Job::Deliver(8));
        assert_eq!(
            drain(&mut q),
            vec![
                Job::Deliver(7),
                Job::Deliver(8),
                Job::Refresh,
                Job::CheckContacts,
                Job::MarkRead(1),
                Job::MarkRead(2),
            ],
            "sends first in the order typed, then the rest in arrival order, reads last"
        );
        assert!(q.is_empty());
    }

    #[test]
    fn start_over_goes_first_and_a_dial_waits_like_a_send() {
        let mut q = CommandQueue::default();
        q.push(Job::Deliver(1));
        q.push(Job::Refresh);
        q.push(Job::Dial);
        q.push(Job::Forget);
        assert_eq!(
            drain(&mut q),
            vec![Job::Forget, Job::Deliver(1), Job::Dial, Job::Refresh]
        );
    }

    #[test]
    fn repeated_refreshes_collapse_but_sends_and_reads_never_do() {
        let mut q = CommandQueue::default();
        q.push(Job::Refresh);
        q.push(Job::Refresh);
        q.push(Job::Deliver(1));
        q.push(Job::Deliver(1));
        q.push(Job::MarkRead(3));
        q.push(Job::MarkRead(3));
        assert_eq!(q.len(), 5, "one refresh; every send and read is kept");
    }

    #[test]
    fn between_refresh_phases_only_sends_are_taken() {
        let mut q = CommandQueue::default();
        q.push(Job::MarkRead(1));
        q.push(Job::Refresh);
        assert!(!q.has_priority());
        assert_eq!(q.take_priority(), None, "a refresh in progress doesn't stop for reads");
        q.push(Job::Deliver(5));
        assert!(q.has_priority());
        assert_eq!(q.take_priority(), Some(Job::Deliver(5)));
        assert_eq!(q.take_priority(), None);
        assert_eq!(drain(&mut q), vec![Job::Refresh, Job::MarkRead(1)]);
    }

    /// A refresh as the worker runs it: phases with a look at the queue between each. A text sent
    /// while the inbox is being listed goes out right after the listing, before the downloads and
    /// the contacts and calls pulls, and ahead of the reads queued before it.
    #[test]
    fn a_send_queued_during_a_refresh_runs_before_the_rest_of_it() {
        #[derive(Debug, PartialEq)]
        enum Step {
            Phase(&'static str),
            Ran(Job),
        }
        let mut q = CommandQueue::default();
        q.push(Job::MarkRead(1));
        let mut log = Vec::new();
        let phases = ["list inbox", "download text 1", "download text 2", "contacts", "calls"];
        for (i, phase) in phases.iter().enumerate() {
            log.push(Step::Phase(phase));
            // The user hits Send while the inbox is being listed.
            if i == 0 {
                q.push(Job::Deliver(9));
            }
            while let Some(job) = q.take_priority() {
                log.push(Step::Ran(job));
            }
        }
        while let Some(job) = q.pop() {
            log.push(Step::Ran(job));
        }
        assert_eq!(
            log,
            vec![
                Step::Phase("list inbox"),
                Step::Ran(Job::Deliver(9)),
                Step::Phase("download text 1"),
                Step::Phase("download text 2"),
                Step::Phase("contacts"),
                Step::Phase("calls"),
                Step::Ran(Job::MarkRead(1)),
            ]
        );
    }
}
