//! A filter-as-you-type list used by `/model`, `/effort`, `/permissions`, `/resume`, `/fork`, `/provider`, `/skills`.

use rig_core::fuzzy::rank_label;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Model,
    Effort,
    Permissions,
    Resume,
    Fork,
    Provider,
    Profile,
    Skills,
    Settings,
}

#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    /// Opaque payload the command handler understands.
    pub value: String,
    pub current: bool,
}

impl PickItem {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            value: value.into(),
            current: false,
        }
    }

    /// A row whose label is also its value.
    pub fn labeled(label: impl Into<String>) -> Self {
        let label = label.into();
        Self::new(label.clone(), label)
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

pub struct Picker {
    pub kind: PickKind,
    pub title: String,
    items: Vec<PickItem>,
    pub filter: String,
    /// (item index, matched label positions), best first.
    filtered: Vec<(usize, Vec<usize>)>,
    pub selected: usize,
}

impl Picker {
    pub fn new(kind: PickKind, title: impl Into<String>, items: Vec<PickItem>) -> Self {
        let mut p = Picker {
            kind,
            title: title.into(),
            items,
            filter: String::new(),
            filtered: vec![],
            selected: 0,
        };
        p.refilter();
        // Start on the current item, like fx's pickers.
        if let Some(pos) = p.filtered.iter().position(|(i, _)| p.items[*i].current) {
            p.selected = pos;
        }
        p
    }

    fn refilter(&mut self) {
        let q = self.filter.trim().to_string();
        let mut scored: Vec<(i32, usize, Vec<usize>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let best = [&it.label, &it.value]
                    .into_iter()
                    .filter_map(|k| rank_label(&q, k))
                    .chain(rank_label(&q, &it.detail).map(|mut m| {
                        m.score -= 500;
                        m.positions.clear();
                        m
                    }))
                    .max_by_key(|m| m.score)?;
                // Highlight only where the label itself matched.
                let positions = rank_label(&q, &it.label).map_or_else(Vec::new, |m| m.positions);
                Some((best.score, i, positions))
            })
            .collect();
        if !q.is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        self.filtered = scored.into_iter().map(|(_, i, p)| (i, p)).collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    /// Replace the items (e.g. when a model list finishes loading) keeping the typed filter.
    pub fn replace_items(&mut self, items: Vec<PickItem>) {
        self.items = items;
        self.refilter();
    }

    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.refilter();
    }

    pub fn nav(&mut self, delta: i32) {
        let n = self.filtered.len() as i32;
        if n > 0 {
            self.selected = (((self.selected as i32 + delta) % n + n) % n) as usize;
        }
    }

    /// Move the highlight to the item whose value is `value`, if it is listed.
    pub fn select_value(&mut self, value: &str) {
        if let Some(pos) = self
            .filtered
            .iter()
            .position(|(i, _)| self.items[*i].value == value)
        {
            self.selected = pos;
        }
    }

    pub fn selected_item(&self) -> Option<&PickItem> {
        self.filtered
            .get(self.selected)
            .map(|(i, _)| &self.items[*i])
    }

    /// Visible rows with highlight positions, windowed around the selection.
    pub fn rows(&self, max: usize) -> Vec<(&PickItem, &[usize], bool)> {
        let start = if self.selected >= max {
            self.selected + 1 - max
        } else {
            0
        };
        self.filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(max)
            .map(|(n, (i, p))| (&self.items[*i], p.as_slice(), n == self.selected))
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.filtered.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }
}

#[cfg(test)]
mod tests;
