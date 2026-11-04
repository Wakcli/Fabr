/// A primary item plus a multi-selection of items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection<T> {
    /// Item shown in the inspector.
    pub primary: Option<T>,
    /// All selected items.
    pub items: Vec<T>,
}

impl<T> Default for Selection<T> {
    fn default() -> Self {
        Self {
            primary: None,
            items: Vec::new(),
        }
    }
}

impl<T: Copy + PartialEq> Selection<T> {
    /// No selection.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A selection holding only `item`.
    pub fn single(item: T) -> Self {
        Self {
            primary: Some(item),
            items: vec![item],
        }
    }

    /// Replaces the selection with `item`.
    pub fn set(&mut self, item: T) {
        self.items.clear();
        self.items.push(item);
        self.primary = Some(item);
    }

    /// Adds `item` when absent, removes it when present.
    pub fn toggle(&mut self, item: T) {
        if let Some(idx) = self.items.iter().position(|it| *it == item) {
            self.items.remove(idx);
            self.primary = self.items.last().copied();
        } else {
            self.items.push(item);
            self.primary = Some(item);
        }
    }

    /// Replaces the selected items; the last one becomes the primary.
    pub fn set_items(&mut self, items: Vec<T>) {
        self.primary = items.last().copied();
        self.items = items;
    }

    /// Drops items `keep` rejects, moving the primary to the last survivor.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.items.retain(|it| keep(it));
        self.primary = self.items.last().copied();
    }

    /// Whether `item` is selected.
    pub fn contains(&self, item: &T) -> bool {
        self.items.iter().any(|it| it == item)
    }

    /// Number of selected items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Every selected item, in selection order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.items.iter().copied()
    }

    /// Drops every selected item.
    pub fn clear(&mut self) {
        self.primary = None;
        self.items.clear();
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_set_replace_contents() {
        let mut sel = Selection::single(3);
        assert_eq!(sel.primary, Some(3));
        assert!(sel.contains(&3));

        sel.set(5);
        assert_eq!(sel.items, vec![5]);
        assert_eq!(sel.primary, Some(5));
        assert!(!sel.contains(&3));
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut sel = Selection::empty();
        assert!(sel.is_empty());

        sel.toggle(1);
        sel.toggle(2);
        assert_eq!(sel.items, vec![1, 2]);
        assert_eq!(sel.primary, Some(2));

        sel.toggle(2);
        assert_eq!(sel.items, vec![1]);
        assert_eq!(sel.primary, Some(1));

        sel.toggle(1);
        assert!(sel.is_empty());
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn clear_empties_the_selection() {
        let mut sel = Selection::single(9);
        sel.toggle(10);
        sel.clear();
        assert!(sel.is_empty());
        assert!(!sel.contains(&9));
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn set_items_keeps_one_primary() {
        let mut sel = Selection::single(1);
        sel.set_items(vec![4, 5, 6]);
        assert_eq!(sel.len(), 3);
        assert_eq!(sel.iter().collect::<Vec<_>>(), vec![4, 5, 6]);
        assert_eq!(sel.primary, Some(6));

        sel.set_items(Vec::new());
        assert!(sel.is_empty());
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn retain_moves_the_primary_to_a_survivor() {
        let mut sel = Selection::empty();
        sel.set_items(vec![1, 2, 3]);
        sel.retain(|it| *it != 3);
        assert_eq!(sel.items, vec![1, 2]);
        assert_eq!(sel.primary, Some(2));

        sel.retain(|_| false);
        assert!(sel.is_empty());
        assert_eq!(sel.primary, None);
    }
}
