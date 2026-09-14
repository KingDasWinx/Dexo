use crate::completion::{CompletionItem, CompletionKind};

/// How many items the popup is worth building. It paints eight rows; anything past a
/// screenful or two is never seen, and the cap is what keeps a catalog with a hundred
/// thousand columns from being sorted for every character typed.
pub const CAP: usize = 50;

/// How well `candidate` answers what the user has typed, or `None` if it does not.
///
/// Four tiers, each one beaten outright by the one above it. dbx scores six -- it splits
/// fuzzy into tight and loose and scores matched initials separately -- but with eight
/// rows on screen those extra tiers decide an order nobody sees. The `- len` term is the
/// part that earns its keep: at equal tier the shorter name wins, so `id` sorts above
/// `identity_provider_id` for "id".
pub fn match_score(candidate: &str, prefix: &str) -> Option<i32> {
    if prefix.is_empty() {
        return Some(0);
    }
    let candidate_lower = candidate.to_ascii_lowercase();
    let prefix_lower = prefix.to_ascii_lowercase();
    let len = candidate.chars().count() as i32;

    if candidate_lower == prefix_lower {
        return Some(3000 - len);
    }
    if candidate_lower.starts_with(&prefix_lower) {
        return Some(2000 - len);
    }
    // `user_id` answers "id": a name is made of words, and any of them can be the one
    // being typed.
    if candidate_lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.starts_with(&prefix_lower))
    {
        return Some(1200 - len);
    }
    subsequence_score(&candidate_lower, &prefix_lower).map(|score| score - len)
}

/// Every character of `prefix`, in order, somewhere in `candidate`. Scored down by how
/// late the first one appears and by how scattered the rest are.
fn subsequence_score(candidate: &str, prefix: &str) -> Option<i32> {
    let mut first = None;
    let mut last = None;
    let mut gaps = 0i32;
    let mut chars = candidate.char_indices();
    for needle in prefix.chars() {
        let (at, _) = chars.find(|(_, ch)| *ch == needle)?;
        if first.is_none() {
            first = Some(at as i32);
        } else if let Some(previous) = last
            && at as i32 > previous + 1
        {
            gaps += 1;
        }
        last = Some(at as i32);
    }
    let first = first?;
    Some(400 + (200 - first * 20).max(0) - gaps * 20)
}

/// Which kind wins when two candidates answer the prefix equally well. A column is what
/// you are most often reaching for once a statement knows its tables; a keyword is what
/// you are least often reaching for, because you already know how to type it.
pub fn kind_priority(kind: CompletionKind) -> i32 {
    match kind {
        CompletionKind::Column => 180,
        CompletionKind::Alias => 170,
        CompletionKind::Table => 160,
        CompletionKind::Function => 90,
        CompletionKind::Snippet => 40,
        CompletionKind::Keyword => 0,
    }
}

/// Extra weight that has nothing to do with spelling.
#[derive(Clone, Copy, Debug, Default)]
pub struct Boosts {
    /// A table the user starred.
    pub favorite: bool,
    /// A table they opened recently.
    pub recent: bool,
    /// `id`, or something ending in `_id` -- the columns a join or a filter is usually
    /// reaching for.
    pub key_column: bool,
}

impl Boosts {
    pub fn total(self) -> i32 {
        let mut total = 0;
        if self.favorite {
            total += 600;
        }
        if self.recent {
            total += 300;
        }
        if self.key_column {
            total += 500;
        }
        total
    }
}

pub fn is_key_column(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "id" || lower.ends_with("_id")
}

/// Orders the list, drops duplicates, and cuts it to `CAP`. Two items are the same when
/// they would insert the same text as the same kind of thing -- a table reachable both
/// by name and by qualified name is one answer, not two.
pub fn finish(mut items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    // Priority is part of the weight, not just a tiebreak: two poor matches are ordered
    // by which kind of thing the position wanted, so a table beats a keyword that merely
    // happens to contain the same letters.
    let weight = |item: &CompletionItem| item.score + kind_priority(item.kind);
    items.sort_by(|a, b| {
        weight(b)
            .cmp(&weight(a))
            .then_with(|| a.label.cmp(&b.label))
    });
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert((item.kind, item.label.clone())));
    items.truncate(CAP);
    items
}
