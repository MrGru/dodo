//! The conversion candidates a host draws in a popup — data only.
//!
//! Nothing in round 1 produces one. Vietnamese has no conversion step: a
//! syllable is unambiguous once its keys are typed, so its
//! [`EngineResult::candidates`](super::EngineResult::candidates) is always
//! `None`.
//!
//! It is here because the languages that *do* have candidates — Japanese
//! kana→kanji, Chinese pinyin→hanzi — need the list to be part of the host
//! protocol from the start, and because pagination is the detail that gets
//! forgotten: a pinyin syllable can have sixty matching characters, shown nine
//! at a time, and an engine that can only hand over a flat `Vec` forces every
//! host to invent its own paging. See [`super::engine`] for the walk-through of
//! how the three CJK engines land on this API.
//!
//! No dictionary, no scoring, no ranking, and no lookup of any kind lives here
//! or in this round.

/// One conversion result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// What is inserted if this one is chosen.
    pub text: String,
    /// A disambiguating hint shown beside it — a reading, a part of speech, a
    /// radical. Never required, never translated by dodo: it comes from the
    /// engine's own data in the engine's own language.
    pub annotation: Option<String>,
}

impl Candidate {
    pub fn new(text: impl Into<String>) -> Candidate {
        Candidate {
            text: text.into(),
            annotation: None,
        }
    }

    pub fn with_annotation(mut self, annotation: impl Into<String>) -> Candidate {
        self.annotation = Some(annotation.into());
        self
    }
}

/// Every candidate for the current composition, plus where the user is in it.
///
/// `selected` indexes [`CandidateList::candidates`] as a whole, not the current
/// page — a list that renumbers its selection when the page turns is the
/// classic off-by-a-page bug in this kind of code.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CandidateList {
    pub candidates: Vec<Candidate>,
    /// Index into `candidates` of the highlighted one.
    pub selected: usize,
    /// Which page is on screen, counted from zero.
    pub page: usize,
    /// How many candidates a page shows. Zero means "all of them, one page".
    pub page_size: usize,
}

impl CandidateList {
    pub fn new(candidates: Vec<Candidate>, page_size: usize) -> CandidateList {
        CandidateList {
            candidates,
            selected: 0,
            page: 0,
            page_size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// How many pages the list has, at least one even when empty.
    pub fn pages(&self) -> usize {
        if self.page_size == 0 {
            return 1;
        }
        self.candidates.len().div_ceil(self.page_size).max(1)
    }

    /// The candidates on the current page.
    pub fn page_slice(&self) -> &[Candidate] {
        if self.page_size == 0 {
            return &self.candidates;
        }
        let start = (self.page * self.page_size).min(self.candidates.len());
        let end = (start + self.page_size).min(self.candidates.len());
        &self.candidates[start..end]
    }

    /// The highlighted candidate, if the list is not empty.
    pub fn selected(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// Move the highlight by `delta`, clamped, and follow it to its page.
    ///
    /// Clamped rather than wrapping: wrapping from the last candidate back to
    /// the first is how a held-down arrow key silently commits the wrong
    /// character.
    pub fn move_selection(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            return;
        }
        let last = self.candidates.len() - 1;
        let next = (self.selected as isize + delta).clamp(0, last as isize) as usize;
        self.selected = next;
        if let Some(page) = next.checked_div(self.page_size) {
            self.page = page;
        }
    }

    /// Turn the page by `delta`, clamped, and pull the highlight onto it.
    pub fn move_page(&mut self, delta: isize) {
        if self.candidates.is_empty() || self.page_size == 0 {
            return;
        }
        let last = self.pages() - 1;
        self.page = (self.page as isize + delta).clamp(0, last as isize) as usize;
        let first = self.page * self.page_size;
        if self.selected < first || self.selected >= first + self.page_size {
            self.selected = first.min(self.candidates.len() - 1);
        }
    }

    /// Highlight the `index`-th candidate *on the current page* — what a number
    /// key does.
    ///
    /// `false` when the page is shorter than that, so a stray `9` does not
    /// commit the last candidate by accident.
    pub fn select_on_page(&mut self, index: usize) -> bool {
        let offset = if self.page_size == 0 {
            0
        } else {
            self.page * self.page_size
        };
        match self.candidates.get(offset + index) {
            Some(_) if index < self.page_slice().len() => {
                self.selected = offset + index;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, CandidateList};

    fn list(count: usize, page_size: usize) -> CandidateList {
        CandidateList::new(
            (0..count).map(|i| Candidate::new(i.to_string())).collect(),
            page_size,
        )
    }

    #[test]
    fn an_empty_list_still_has_one_page_and_no_selection() {
        let empty = CandidateList::default();
        assert!(empty.is_empty());
        assert_eq!(empty.pages(), 1);
        assert_eq!(empty.selected(), None);
        assert!(empty.page_slice().is_empty());
    }

    #[test]
    fn pagination_splits_the_list_without_renumbering_the_selection() {
        let mut list = list(20, 9);
        assert_eq!(list.pages(), 3);
        assert_eq!(list.page_slice().len(), 9);

        list.move_selection(10);
        assert_eq!(list.selected, 10);
        assert_eq!(list.page, 1);
        assert_eq!(list.selected().unwrap().text, "10");

        // The last page is short; the slice must not run past the end.
        list.move_page(1);
        assert_eq!(list.page, 2);
        assert_eq!(list.page_slice().len(), 2);
    }

    /// A held-down arrow must stop at the end, not wrap round to the first
    /// candidate and commit something the user never looked at.
    #[test]
    fn selection_clamps_rather_than_wrapping() {
        let mut list = list(5, 9);
        list.move_selection(-3);
        assert_eq!(list.selected, 0);
        list.move_selection(99);
        assert_eq!(list.selected, 4);
    }

    #[test]
    fn paging_pulls_the_selection_onto_the_visible_page() {
        let mut list = list(20, 9);
        list.move_page(2);
        assert_eq!(list.page, 2);
        assert_eq!(list.selected, 18);
        list.move_page(-1);
        assert_eq!(list.page, 1);
        assert_eq!(list.selected, 9);
        // Already clamped at the top.
        list.move_page(-9);
        assert_eq!(list.page, 0);
    }

    #[test]
    fn a_number_key_selects_within_the_visible_page_only() {
        let mut list = list(20, 9);
        list.move_page(2);
        assert!(list.select_on_page(1));
        assert_eq!(list.selected().unwrap().text, "19");
        // The last page holds two; `3` names nothing.
        assert!(!list.select_on_page(3));
        assert_eq!(list.selected().unwrap().text, "19");
    }

    #[test]
    fn page_size_zero_means_one_page_of_everything() {
        let mut list = list(30, 0);
        assert_eq!(list.pages(), 1);
        assert_eq!(list.page_slice().len(), 30);
        list.move_page(4);
        assert_eq!(list.page, 0);
        assert!(list.select_on_page(29));
        assert_eq!(list.selected, 29);
    }

    #[test]
    fn an_annotation_is_optional_and_carried_verbatim() {
        let candidate = Candidate::new("漢").with_annotation("kan");
        assert_eq!(candidate.text, "漢");
        assert_eq!(candidate.annotation.as_deref(), Some("kan"));
        assert_eq!(Candidate::new("字").annotation, None);
    }
}
