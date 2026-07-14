use std::collections::HashMap;

fn word_counts(text: &str) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for raw in text.split_whitespace() {
        // Keep only alphanumeric, lowercase; skip anything that empties out.
        let word: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if word.is_empty() {
            continue;
        }
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}

fn top_n(counts: &HashMap<String, usize>, n: usize) -> Vec<(String, usize)> {
    let mut pairs: Vec<(String, usize)> = counts
        .iter()
        .map(|(word, count)| (word.clone(), *count))
        .collect();
    // Highest count first; ties broken alphabetically for deterministic output.
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.truncate(n);
    pairs
}

fn main() {
    let text = "the quick brown fox the lazy dog THE Fox jumps over the lazy dog";
    let counts = word_counts(text);
    println!("total distinct words: {}", counts.len());
    for (word, count) in top_n(&counts, 3) {
        println!("{count:>3}  {word}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_normalizes_case() {
        let counts = word_counts("The the THE fox");
        assert_eq!(counts.get("the"), Some(&3));
        assert_eq!(counts.get("fox"), Some(&1));
    }

    #[test]
    fn ranks_by_count_then_alphabetically() {
        let counts = word_counts("b a a b b c");
        let top = top_n(&counts, 2);
        assert_eq!(top, vec![("b".to_string(), 3), ("a".to_string(), 2)]);
    }
}
