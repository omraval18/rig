//! The filter-as-you-type list behind every chooser: models, providers, profiles, sessions, settings, help.

use rig_core::fuzzy::rank_label;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Model,
    Effort,
    Permissions,
    Resume,
    Fork,
    Provider,
    /// What to do with a connected provider.
    ProviderAction,
    Profile,
    ProfileRemove,
    ProviderKind,
    CatalogMatch,
    Skills,
    Settings,
    Help,
}

impl PickKind {
    /// What the keys do here, beyond typing to filter.
    pub fn hint(self) -> &'static str {
        match self {
            PickKind::Model => "↑↓ move · ←→ provider · Ctrl+R refresh · Enter select · Esc cancel",
            PickKind::Profile => {
                "↑↓ move · type a new name to create it · Enter select · Esc cancel"
            }
            PickKind::Provider => {
                "type to search · ↑↓ move · ←→ section · Enter select · Esc cancel"
            }
            PickKind::Help => "type to search · ↑↓ move · ←→ section · Enter run · Esc close",
            _ => "type to filter · ↑↓ move · Enter select · Esc cancel",
        }
    }
}

/// Rows offered for text that names nothing in the list.
#[derive(Clone, Debug)]
pub enum Create {
    /// A profile by that name, fresh or with this profile's keys and settings.
    Profile { copy_from: String },
    /// A custom endpoint: a URL typed in the provider list, or nothing there matching.
    Endpoint,
    /// A model id the list does not have.
    Model,
}

/// Marks the value of a [`Create`] row; no ordinary value contains it.
pub const CREATE: char = '\t';

impl Create {
    fn rows(&self, typed: &str, no_match: bool) -> Vec<PickItem> {
        match self {
            Create::Profile { copy_from } => {
                if crate::profile::validate(typed).is_err() {
                    return Vec::new();
                }
                vec![
                    PickItem::new(format!("+ Create '{typed}'"), format!("{CREATE}{typed}"))
                        .detail("fresh: set up its providers next"),
                    PickItem::new(
                        format!("+ Create '{typed}' from '{copy_from}'"),
                        format!("{CREATE}{typed}{CREATE}{copy_from}"),
                    )
                    .detail("copies keys, providers and settings; not the ChatGPT login"),
                ]
            }
            Create::Endpoint if typed.contains("://") => vec![
                PickItem::new(format!("+ Connect {typed}"), format!("{CREATE}{typed}"))
                    .detail("custom endpoint"),
            ],
            Create::Endpoint if no_match => vec![
                PickItem::new("+ Custom endpoint…", CREATE.to_string())
                    .detail(format!("'{typed}' is not listed: connect it by URL")),
            ],
            Create::Endpoint => Vec::new(),
            Create::Model => vec![
                PickItem::new(format!("Use '{typed}'"), format!("{CREATE}{typed}"))
                    .detail("exactly as typed"),
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    /// Opaque payload the command handler understands.
    pub value: String,
    pub current: bool,
    /// Heading this row is under. Rows of one group are adjacent.
    pub group: String,
}

impl PickItem {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            value: value.into(),
            current: false,
            group: String::new(),
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

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
}

/// One drawn line of the list.
pub enum Row<'a> {
    Heading(&'a str),
    Item {
        item: &'a PickItem,
        positions: &'a [usize],
        selected: bool,
    },
}

pub struct Picker {
    pub kind: PickKind,
    pub title: String,
    items: Vec<PickItem>,
    pub filter: String,
    /// (item index, matched label positions), best first.
    filtered: Vec<(usize, Vec<usize>)>,
    pub selected: usize,
    create: Option<Create>,
    /// Items before the rows `create` added for the current filter.
    given: usize,
    /// The one provider whose models are listed, when not all of them are.
    pub scope: Option<String>,
}

/// How well `item` matches every word of `words`, and the label positions to highlight.
fn rank(words: &[&str], item: &PickItem) -> Option<(i32, Vec<usize>)> {
    let (mut score, mut positions) = (0, Vec::new());
    for word in words {
        let label = rank_label(word, &item.label);
        let best = label
            .iter()
            .map(|m| m.score)
            .chain(rank_label(word, &item.value).map(|m| m.score))
            .chain(rank_label(word, &item.detail).map(|m| m.score - 500))
            .chain(rank_label(word, &item.group).map(|m| m.score - 500))
            .max()?;
        score += best;
        positions.extend(label.into_iter().flat_map(|m| m.positions));
    }
    Some((score, positions))
}

impl Picker {
    pub fn new(kind: PickKind, title: impl Into<String>, items: Vec<PickItem>) -> Self {
        let mut p = Picker {
            kind,
            title: title.into(),
            given: items.len(),
            items,
            filter: String::new(),
            filtered: vec![],
            selected: 0,
            create: None,
            scope: None,
        };
        p.refilter();
        p.select_current();
        p
    }

    /// Start on the current item.
    fn select_current(&mut self) {
        if let Some(pos) = self
            .filtered
            .iter()
            .position(|(i, _)| self.items[*i].current)
        {
            self.selected = pos;
        }
    }

    pub fn set_create(&mut self, create: Create) {
        self.create = Some(create);
        self.refilter();
    }

    fn refilter(&mut self) {
        self.items.truncate(self.given);
        let q = self.filter.trim().to_string();
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut scored: Vec<(i32, usize, Vec<usize>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| rank(&words, it).map(|(score, p)| (score, i, p)))
            .collect();
        if !q.is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        self.filtered = scored.into_iter().map(|(_, i, p)| (i, p)).collect();
        if let Some(create) = &self.create
            && !q.is_empty()
            && !self.items.iter().any(|i| i.value == q)
        {
            for row in create.rows(&q, self.filtered.is_empty()) {
                self.filtered.push((self.items.len(), Vec::new()));
                self.items.push(row);
            }
        }
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    /// Replace the items (e.g. when a model list finishes loading), keeping the typed filter and
    /// the highlighted item.
    pub fn replace_items(&mut self, items: Vec<PickItem>) {
        let highlighted = self.selected_item().map(|i| i.value.clone());
        self.given = items.len();
        self.items = items;
        self.refilter();
        if let Some(value) = highlighted {
            self.select_value(&value);
        }
    }

    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.to_string();
        self.selected = 0;
        self.refilter();
        if self.filter.is_empty() {
            self.select_current();
        }
    }

    /// Pasted text joins the filter as one line.
    pub fn paste(&mut self, text: &str) {
        let add: String = text.trim().chars().filter(|c| !c.is_control()).collect();
        self.set_filter(&format!("{}{add}", self.filter));
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

    fn group_at(&self, pos: usize) -> &str {
        self.filtered
            .get(pos)
            .map_or("", |(i, _)| self.items[*i].group.as_str())
    }

    /// Move to the first row of the next (or previous) group.
    pub fn nav_group(&mut self, forward: bool) {
        if !self.filter.trim().is_empty() {
            return;
        }
        let starts: Vec<usize> = (0..self.filtered.len())
            .filter(|p| *p == 0 || self.group_at(*p) != self.group_at(*p - 1))
            .collect();
        let next = if forward {
            starts
                .iter()
                .find(|p| **p > self.selected)
                .or(starts.first())
        } else {
            let here = starts.iter().rposition(|p| *p <= self.selected);
            here.and_then(|h| h.checked_sub(1))
                .and_then(|h| starts.get(h))
                .or(starts.last())
        };
        if let Some(p) = next {
            self.selected = *p;
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

    /// `3/57`: where the highlight is among the matches.
    pub fn position(&self) -> String {
        let at = (self.selected + 1).min(self.filtered.len());
        format!("{at}/{}", self.filtered.len())
    }

    /// At most `max` lines around the selection. Headings show while nothing is typed; a filtered
    /// list is ordered by match instead.
    pub fn rows(&self, max: usize) -> Vec<Row<'_>> {
        let grouped = self.filter.trim().is_empty();
        let mut all = Vec::with_capacity(self.filtered.len());
        let mut at = 0;
        for (n, (i, positions)) in self.filtered.iter().enumerate() {
            let item = &self.items[*i];
            let new_group = n == 0 || self.group_at(n - 1) != item.group;
            if grouped && new_group && !item.group.is_empty() {
                all.push(Row::Heading(&item.group));
            }
            if n == self.selected {
                at = all.len();
            }
            all.push(Row::Item {
                item,
                positions,
                selected: n == self.selected,
            });
        }
        let start = (at + 1).saturating_sub(max);
        all.into_iter().skip(start).take(max).collect()
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
