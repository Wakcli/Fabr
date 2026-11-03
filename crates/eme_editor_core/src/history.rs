use core::any::Any;

/// Number of undo entries kept when no other limit is given.
pub const DEFAULT_LIMIT: usize = 200;

/// A reversible mutation of the document state.
pub trait Op<S> {
    /// Label shown in undo UI.
    fn name(&self) -> &str;

    /// Applies the mutation.
    fn apply(&mut self, state: &mut S);

    /// Reverses the mutation.
    fn revert(&mut self, state: &mut S);

    /// Absorbs `next` into this op when the two can be coalesced.
    fn try_merge(&mut self, _next: &dyn Op<S>) -> bool {
        false
    }

    /// This op as [`Any`], so merge implementations can inspect the next op.
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }
}

/// Undo and redo stacks over a document state.
pub struct History<S> {
    undo: Vec<Box<dyn Op<S>>>,
    redo: Vec<Box<dyn Op<S>>>,
    limit: usize,
}

impl<S> History<S> {
    /// A history keeping at most `limit` entries; `0` records nothing.
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    /// Applies `op`, coalescing it into the top entry when possible.
    pub fn push(&mut self, state: &mut S, mut op: Box<dyn Op<S>>) {
        op.apply(state);
        self.redo.clear();
        if self.limit == 0 {
            return;
        }
        if let Some(top) = self.undo.last_mut()
            && top.try_merge(op.as_ref())
        {
            return;
        }
        self.undo.push(op);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Reverts the newest op and returns its name.
    pub fn undo(&mut self, state: &mut S) -> Option<String> {
        let mut op = self.undo.pop()?;
        op.revert(state);
        let name = op.name().to_string();
        self.redo.push(op);
        Some(name)
    }

    /// Re-applies the most recently undone op and returns its name.
    pub fn redo(&mut self, state: &mut S) -> Option<String> {
        let mut op = self.redo.pop()?;
        op.apply(state);
        let name = op.name().to_string();
        self.undo.push(op);
        Some(name)
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Name of the newest undoable op.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|op| op.name())
    }

    /// Name of the newest redoable op.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|op| op.name())
    }

    /// Number of recorded undo entries.
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    /// Whether no undo entries are recorded.
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// Drops both stacks.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

impl<S> Default for History<S> {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i32);

    impl Op<i32> for Add {
        fn name(&self) -> &str {
            "add"
        }

        fn apply(&mut self, state: &mut i32) {
            *state += self.0;
        }

        fn revert(&mut self, state: &mut i32) {
            *state -= self.0;
        }

        fn try_merge(&mut self, next: &dyn Op<i32>) -> bool {
            let Some(next) = next.as_any().and_then(|a| a.downcast_ref::<Add>()) else {
                return false;
            };
            self.0 += next.0;
            true
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    struct Set {
        value: i32,
        before: i32,
    }

    impl Set {
        fn new(value: i32) -> Self {
            Self { value, before: 0 }
        }
    }

    impl Op<i32> for Set {
        fn name(&self) -> &str {
            "set"
        }

        fn apply(&mut self, state: &mut i32) {
            self.before = *state;
            *state = self.value;
        }

        fn revert(&mut self, state: &mut i32) {
            *state = self.before;
        }
    }

    #[test]
    fn undo_and_redo_follow_stack_order() {
        let mut state = 0;
        let mut history = History::default();

        history.push(&mut state, Box::new(Add(2)));
        history.push(&mut state, Box::new(Set::new(10)));
        assert_eq!(state, 10);
        assert_eq!(history.len(), 2);

        assert_eq!(history.undo(&mut state), Some("set".into()));
        assert_eq!(state, 2);
        assert_eq!(history.undo(&mut state), Some("add".into()));
        assert_eq!(state, 0);
        assert_eq!(history.undo(&mut state), None);

        assert_eq!(history.redo(&mut state), Some("add".into()));
        assert_eq!(state, 2);
        assert_eq!(history.redo(&mut state), Some("set".into()));
        assert_eq!(state, 10);
        assert_eq!(history.redo(&mut state), None);
        assert!(!history.can_redo());
        assert!(history.can_undo());
        assert_eq!(history.undo_name(), Some("set"));
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn merge_coalesces_the_top_entry() {
        let mut state = 0;
        let mut history = History::default();

        history.push(&mut state, Box::new(Add(2)));
        history.push(&mut state, Box::new(Add(3)));

        assert_eq!(state, 5);
        assert_eq!(history.len(), 1);
        assert_eq!(history.undo(&mut state), Some("add".into()));
        assert_eq!(state, 0);
        assert!(!history.can_undo());
    }

    #[test]
    fn mismatched_ops_do_not_merge() {
        let mut state = 0;
        let mut history = History::default();

        history.push(&mut state, Box::new(Add(2)));
        history.push(&mut state, Box::new(Set::new(7)));
        history.push(&mut state, Box::new(Add(1)));

        assert_eq!(history.len(), 3);
        assert_eq!(state, 8);
    }

    #[test]
    fn new_push_clears_redo() {
        let mut state = 0;
        let mut history = History::default();

        history.push(&mut state, Box::new(Add(2)));
        history.undo(&mut state);
        assert!(history.can_redo());

        history.push(&mut state, Box::new(Add(5)));
        assert!(!history.can_redo());
        assert_eq!(history.redo_name(), None);
        assert_eq!(state, 5);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn limit_evicts_oldest() {
        let mut state = 0;
        let mut history = History::new(3);

        for i in 1..=5 {
            history.push(&mut state, Box::new(Set::new(i)));
        }

        assert_eq!(history.len(), 3);
        assert_eq!(state, 5);

        history.undo(&mut state);
        history.undo(&mut state);
        history.undo(&mut state);
        assert_eq!(state, 2);
        assert!(!history.can_undo());
    }

    #[test]
    fn zero_limit_applies_but_records_nothing() {
        let mut state = 0;
        let mut history = History::new(0);
        history.push(&mut state, Box::new(Add(4)));
        assert_eq!(state, 4);
        assert!(history.is_empty());
        assert!(!history.can_undo());
    }

    #[test]
    fn clear_drops_both_stacks() {
        let mut state = 0;
        let mut history = History::default();
        history.push(&mut state, Box::new(Add(1)));
        history.undo(&mut state);

        history.clear();
        assert!(history.is_empty());
        assert!(!history.can_redo());
    }
}
