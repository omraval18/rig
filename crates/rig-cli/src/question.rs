//! A question from the agent: options to pick from, and a last row where the person types an
//! answer of their own. The options are suggestions; the typed row is always there.

pub struct Question {
    pub text: String,
    pub options: Vec<String>,
    pub selected: usize,
    /// Some while the person is typing their own answer.
    pub typing: Option<String>,
}

impl Question {
    /// With no options there is nothing to pick, so the answer is typed straight away.
    pub fn new(text: String, options: Vec<String>) -> Self {
        let typing = options.is_empty().then(String::new);
        Self {
            text,
            options,
            selected: 0,
            typing,
        }
    }

    /// Every row: the options, then the typed-answer row.
    pub fn rows(&self) -> usize {
        self.options.len() + 1
    }

    /// The row that opens the typing box.
    pub fn own_row(&self) -> usize {
        self.options.len()
    }

    pub fn nav(&mut self, delta: isize) {
        let n = self.rows() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typed_row_is_always_last_and_wraps() {
        let mut q = Question::new("Which?".into(), vec!["a".into(), "b".into()]);
        assert_eq!((q.rows(), q.own_row()), (3, 2));
        q.nav(-1);
        assert_eq!(q.selected, 2);
        q.nav(1);
        assert_eq!(q.selected, 0);
        assert!(q.typing.is_none());
    }

    #[test]
    fn without_options_the_answer_is_typed_at_once() {
        let q = Question::new("Name?".into(), Vec::new());
        assert_eq!(q.typing.as_deref(), Some(""));
    }
}
