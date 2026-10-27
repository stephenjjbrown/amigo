//! Word-level diffing, used to present several translations of one sentence as
//! edits against the most representative one.

/// One step of a token diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffOp {
    Keep(String),
    /// A token the base has and the variant does not.
    Delete(String),
    /// A token the variant has and the base does not.
    Insert(String),
}

/// Split on whitespace, keeping punctuation attached to its word. Good enough
/// for comparing short translations; it deliberately does not try to be a
/// tokenizer for the model.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// Longest common subsequence table, computed from the end so it can be walked
/// forward while emitting operations.
fn lcs_table(base: &[String], other: &[String]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0; other.len() + 1]; base.len() + 1];

    for i in (0..base.len()).rev() {
        for j in (0..other.len()).rev() {
            table[i][j] = if base[i] == other[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    table
}

/// Tokens the two share, in order.
pub fn lcs_len(base: &[String], other: &[String]) -> usize {
    lcs_table(base, other)[0][0]
}

/// How many token edits separate two variants.
pub fn token_distance(base: &[String], other: &[String]) -> usize {
    base.len() + other.len() - 2 * lcs_len(base, other)
}

/// Diff `other` against `base`. Deletions are emitted before insertions when
/// both are possible, so a replaced phrase reads as "old then new".
pub fn diff(base: &[String], other: &[String]) -> Vec<DiffOp> {
    let table = lcs_table(base, other);
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < base.len() && j < other.len() {
        if base[i] == other[j] {
            ops.push(DiffOp::Keep(base[i].clone()));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            ops.push(DiffOp::Delete(base[i].clone()));
            i += 1;
        } else {
            ops.push(DiffOp::Insert(other[j].clone()));
            j += 1;
        }
    }

    while i < base.len() {
        ops.push(DiffOp::Delete(base[i].clone()));
        i += 1;
    }
    while j < other.len() {
        ops.push(DiffOp::Insert(other[j].clone()));
        j += 1;
    }

    ops
}

/// Index of the variant with the smallest total distance to the others, so the
/// diffs are drawn against a typical sentence rather than an outlier.
pub fn medoid(variants: &[Vec<String>]) -> usize {
    let mut best = 0;
    let mut best_total = usize::MAX;

    for (index, candidate) in variants.iter().enumerate() {
        let total: usize = variants
            .iter()
            .map(|other| token_distance(candidate, other))
            .sum();
        if total < best_total {
            best_total = total;
            best = index;
        }
    }

    best
}

/// One distinct translation and how many samples produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub text: String,
    pub count: usize,
    /// Index of the first sample with this text.
    pub first_index: usize,
}

/// Collapse identical samples, keeping the order they first appeared in.
pub fn group(samples: &[&str]) -> Vec<Variant> {
    let mut groups: Vec<Variant> = Vec::new();

    for (index, sample) in samples.iter().enumerate() {
        let text = sample.trim().to_string();
        match groups.iter_mut().find(|group| group.text == text) {
            Some(group) => group.count += 1,
            None => groups.push(Variant {
                text,
                count: 1,
                first_index: index,
            }),
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<String> {
        tokenize(text)
    }

    #[test]
    fn keeps_identical_sentences() {
        let base = tokens("Como vai você");
        assert_eq!(
            diff(&base, &base),
            vec![
                DiffOp::Keep("Como".into()),
                DiffOp::Keep("vai".into()),
                DiffOp::Keep("você".into()),
            ]
        );
    }

    #[test]
    fn reports_replaced_tokens() {
        let base = tokens("Gostaria de ver o menu.");
        let other = tokens("Queria ver a ementa.");
        let ops = diff(&base, &other);

        assert!(ops.contains(&DiffOp::Insert("Queria".into())));
        assert!(ops.contains(&DiffOp::Insert("ementa.".into())));
        assert!(ops.contains(&DiffOp::Delete("Gostaria".into())));
        assert!(ops.contains(&DiffOp::Keep("ver".into())));
    }

    #[test]
    fn reordering_reads_as_delete_and_insert() {
        let base = tokens("Como vai você");
        let other = tokens("Como você está");
        let ops = diff(&base, &other);

        assert_eq!(ops.first(), Some(&DiffOp::Keep("Como".into())));
        assert!(ops.contains(&DiffOp::Delete("vai".into())));
        assert!(ops.contains(&DiffOp::Insert("está".into())));
        assert!(ops.contains(&DiffOp::Keep("você".into())));
    }

    #[test]
    fn medoid_prefers_the_middle_variant() {
        let variants = vec![
            tokens("a b c"),
            tokens("a b c d"),
            tokens("a b c d e"),
            tokens("a b c d e f"),
            tokens("q"),
        ];

        assert_eq!(medoid(&variants), 1);
    }

    #[test]
    fn groups_duplicate_samples() {
        let samples = ["um", "dois", "um", " três "];
        let groups = group(&samples);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].text, "um");
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[0].first_index, 0);
        assert_eq!(groups[2].text, "três");
        assert_eq!(groups[2].first_index, 3);
    }
}
