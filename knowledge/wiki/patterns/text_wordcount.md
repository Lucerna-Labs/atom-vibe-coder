# HashMap counting with iterator chains and stable top-N sorting
tags: text, tokenize, wordcount, frequency, hashmap, iterator, sort, collections, stdin, dependency-free

A reference architecture for a small std-only Rust program that reads text, counts occurrences of each distinct token, and prints the most common tokens ranked by frequency. The reference implementation is a word-frequency counter, but the same shape works for any tally-then-rank problem — log-level counts, distinct-IP tallies, histogram-of-strings tasks, and so on.

## Structural pattern

- **`HashMap<String, usize>` is the counting table.** The `.entry(key).or_insert(0)` idiom is the smallest correct way to increment. Never use `contains_key` followed by `insert`; that is two hash lookups instead of one.
- **Iterator chain for normalisation.** Split the input on whitespace or punctuation, filter out empty tokens, `.to_ascii_lowercase()` where the domain requires it, and feed each surviving token into the count table. The chain is fluent and there is no intermediate `Vec`.
- **Stable sort by count-descending, then key-ascending.** Collect the map into a `Vec<(&String, &usize)>` and call `.sort_by` with a tuple key `(std::cmp::Reverse(count), token)` so ties break deterministically. This is the pattern to reach for whenever the output has to be reproducible.
- **`take(n)` truncates.** After sorting, take the top N with an iterator method — do not resize the whole Vec.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a wordcount, histogram, distinct-value tally, "most common X", or a "which appears more" pairwise comparison of two corpora. The tokeniser is what varies; the count-then-sort skeleton is the reusable structure.

## Anti-patterns to avoid

Do not build the map with `insert` when `entry` is available. Do not sort by count alone — ties will re-order across runs. Do not push tokens into a `Vec` before counting; the whole point of `HashMap` is to avoid that intermediate allocation.

## Related

[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/text_wordcount.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
