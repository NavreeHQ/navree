//! [`Chunker`]: long text split into passages that embed well.

/// A passage of a split text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// Its position, from 0.
    pub index: usize,
    /// The passage (for Markdown, its heading path first).
    pub text: String,
}

/// Splits text at paragraph, then sentence, then word boundaries into
/// chunks of at most `max_tokens`, each starting with the last `overlap`
/// tokens of the one before, so a fact that straddles a boundary is in
/// both. Tokens are counted as four characters each.
///
/// ```
/// use navree_ai::Chunker;
///
/// let text = "First paragraph. It has two sentences.\n\nSecond paragraph.";
/// let chunks = Chunker::new().max_tokens(300).overlap(40).split(text);
/// assert_eq!(chunks.len(), 1); // shorter than max_tokens: one chunk
/// assert_eq!(chunks[0].index, 0);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunker {
    max_tokens: usize,
    overlap: usize,
    markdown: bool,
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker {
    /// A plain-text chunker: 512 tokens per chunk, 64 of overlap.
    pub fn new() -> Self {
        Self {
            max_tokens: 512,
            overlap: 64,
            markdown: false,
        }
    }

    /// A Markdown chunker: splits at headings first and starts each chunk
    /// with its heading path (`Pricing > Discounts`).
    pub fn markdown() -> Self {
        Self {
            markdown: true,
            ..Self::new()
        }
    }

    /// At most `max_tokens` per chunk (at least 1).
    pub fn max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens.max(1);
        self
    }

    /// Tokens repeated from the end of the previous chunk (less than
    /// `max_tokens`).
    pub fn overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// The chunks of `text`; empty for blank text.
    pub fn split(&self, text: &str) -> Vec<Chunk> {
        let pieces: Vec<String> = if self.markdown {
            sections(text)
                .into_iter()
                .flat_map(|(path, body)| {
                    let prefix = if path.is_empty() {
                        String::new()
                    } else {
                        format!("{path}\n\n")
                    };
                    let budget = self.max_tokens.saturating_sub(tokens(&prefix)).max(1);
                    split_plain(&body, budget, self.overlap.min(budget.saturating_sub(1)))
                        .into_iter()
                        .map(move |piece| format!("{prefix}{piece}"))
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            split_plain(
                text,
                self.max_tokens,
                self.overlap.min(self.max_tokens.saturating_sub(1)),
            )
        };
        pieces
            .into_iter()
            .enumerate()
            .map(|(index, text)| Chunk { index, text })
            .collect()
    }
}

/// Tokens in `text`: four characters each, rounded up.
fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// A piece of text and the separator that follows it.
#[derive(Clone, Debug)]
struct Unit {
    text: String,
    separator: &'static str,
}

/// Paragraphs, split into sentences (and lines), split into word runs
/// that fit; each unit keeps the separator that followed it.
fn units(text: &str, max: usize) -> Vec<Unit> {
    let mut out = Vec::new();
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    for paragraph in paragraphs {
        let sentences = sentences(paragraph);
        let count = sentences.len();
        for (i, (sentence, after)) in sentences.into_iter().enumerate() {
            let separator = if i + 1 == count { "\n\n" } else { after };
            if tokens(&sentence) <= max {
                out.push(Unit {
                    text: sentence,
                    separator,
                });
                continue;
            }
            let words = word_runs(&sentence, max);
            let n = words.len();
            for (j, run) in words.into_iter().enumerate() {
                out.push(Unit {
                    text: run,
                    separator: if j + 1 == n { separator } else { " " },
                });
            }
        }
    }
    out
}

/// The sentences of a paragraph (ends: `.`, `!`, `?` before a space, or a
/// line break), each with what followed it (`" "` or `"\n"`).
fn sentences(paragraph: &str) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = paragraph.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            if !current.trim().is_empty() {
                out.push((current.trim().to_owned(), "\n"));
            }
            current.clear();
            continue;
        }
        current.push(*c);
        let next = chars.get(i + 1);
        if matches!(c, '.' | '!' | '?') && next.is_none_or(|n| n.is_whitespace()) {
            if !current.trim().is_empty() {
                let after = if next == Some(&'\n') { "\n" } else { " " };
                out.push((current.trim().to_owned(), after));
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push((current.trim().to_owned(), " "));
    }
    out
}

/// Words joined into runs of at most `max` tokens (a longer word is cut).
fn word_runs(sentence: &str, max: usize) -> Vec<String> {
    let max_chars = max * 4;
    let mut out = Vec::new();
    let mut current = String::new();
    for word in sentence.split_whitespace() {
        let mut word = word.to_owned();
        while word.chars().count() > max_chars {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            let head: String = word.chars().take(max_chars).collect();
            word = word.chars().skip(max_chars).collect();
            out.push(head);
        }
        let candidate = if current.is_empty() {
            word.clone()
        } else {
            format!("{current} {word}")
        };
        if tokens(&candidate) > max && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current = word;
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Joins units back into text.
fn join(units: &[Unit]) -> String {
    let mut text = String::new();
    for (i, unit) in units.iter().enumerate() {
        text.push_str(&unit.text);
        if i + 1 < units.len() {
            text.push_str(unit.separator);
        }
    }
    text
}

/// Greedy packing of units into chunks with overlap.
fn split_plain(text: &str, max: usize, overlap: usize) -> Vec<String> {
    let units = units(text, max);
    if units.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current: Vec<Unit> = Vec::new();
    let mut fresh = 0; // units in `current` not carried over by overlap
    for unit in units {
        let mut candidate = current.clone();
        candidate.push(unit.clone());
        if tokens(&join(&candidate)) <= max {
            current = candidate;
            fresh += 1;
            continue;
        }
        if fresh > 0 {
            chunks.push(join(&current));
        }
        // Carry the tail of the chunk that ended, within `overlap` tokens
        // and leaving room for the new unit.
        let mut carried: Vec<Unit> = Vec::new();
        for previous in current.iter().rev() {
            let mut trial = vec![previous.clone()];
            trial.extend(carried.iter().cloned());
            let mut with_new = trial.clone();
            with_new.push(unit.clone());
            if tokens(&join(&trial)) > overlap || tokens(&join(&with_new)) > max {
                break;
            }
            carried = trial;
        }
        carried.push(unit);
        current = carried;
        fresh = 1;
    }
    if fresh > 0 {
        chunks.push(join(&current));
    }
    chunks
}

/// Markdown sections: (heading path, body), in order.
fn sections(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut path: Vec<(usize, String)> = Vec::new();
    let mut body = String::new();
    let mut in_fence = false;
    let flush = |path: &[(usize, String)], body: &mut String, out: &mut Vec<(String, String)>| {
        if !body.trim().is_empty() {
            let heading = path
                .iter()
                .map(|(_, h)| h.as_str())
                .collect::<Vec<_>>()
                .join(" > ");
            out.push((heading, body.trim().to_owned()));
        }
        body.clear();
    };
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        let is_heading = !in_fence
            && (1..=6).contains(&level)
            && line.chars().nth(level).is_some_and(char::is_whitespace);
        if is_heading {
            flush(&path, &mut body, &mut out);
            let title = line[level..].trim().to_owned();
            while path.last().is_some_and(|(l, _)| *l >= level) {
                path.pop();
            }
            path.push((level, title));
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&path, &mut body, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let chunks = Chunker::new()
            .max_tokens(300)
            .split("Pricing call with Daljit.");
        assert_eq!(
            chunks,
            [Chunk {
                index: 0,
                text: "Pricing call with Daljit.".into()
            }]
        );
        assert!(Chunker::new().split("   \n\n ").is_empty());
    }

    #[test]
    fn long_text_splits_at_sentences_within_the_limit() {
        let sentence = "The customer asked about the onboarding timeline again.";
        let text = vec![sentence; 20].join(" ");
        let chunks = Chunker::new().max_tokens(40).overlap(0).split(&text);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(tokens(&chunk.text) <= 40, "{}", chunk.text);
            assert!(chunk.text.ends_with('.'), "{}", chunk.text);
        }
        assert_eq!(
            chunks.iter().map(|c| c.index).collect::<Vec<_>>(),
            (0..chunks.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn neighbors_overlap() {
        let text = (1..=12)
            .map(|i| format!("Sentence number {i} is here."))
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = Chunker::new().max_tokens(20).overlap(8).split(&text);
        assert!(chunks.len() > 2);
        for pair in chunks.windows(2) {
            let last_of_first = pair[0]
                .text
                .rsplit(". ")
                .next()
                .unwrap()
                .trim_end_matches('.');
            assert!(pair[1].text.contains(last_of_first), "{:?}", pair);
        }
    }

    #[test]
    fn very_long_words_are_cut() {
        let text = "x".repeat(100);
        let chunks = Chunker::new().max_tokens(5).overlap(0).split(&text);
        assert_eq!(chunks.len(), 5);
        assert!(chunks.iter().all(|c| c.text.len() == 20));
    }

    #[test]
    fn markdown_keeps_the_heading_path() {
        let text = "Intro line.\n\n# Pricing\n\nList prices.\n\n## Discounts\n\nTen percent for annual terms.\n\n# Support\n\nEmail only.";
        let chunks = Chunker::markdown().max_tokens(100).split(text);
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Intro line.",
                "Pricing\n\nList prices.",
                "Pricing > Discounts\n\nTen percent for annual terms.",
                "Support\n\nEmail only."
            ]
        );
    }

    #[test]
    fn markdown_ignores_hashes_in_code() {
        let text = "# Setup\n\n```sh\n# a comment\nrun\n```";
        let chunks = Chunker::markdown().split(text);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("Setup\n\n```sh\n# a comment"));
    }
}
