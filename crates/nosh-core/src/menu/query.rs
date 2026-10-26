//! Client-side filtering and ordering for `search = "client"` sources.

use super::model::MenuItem;

/// A query longer than this is truncated. Nobody types a useful 256 character
/// search and long input should not cost scanning time.
const MAX_QUERY_CHARS: usize = 256;

/// Scores a case-insensitive match. Returns `None` when the query cannot match.
///
/// Three kinds of match are possible, strongest first:
///
/// 1. A contiguous substring. An exact match in the middle of a word still
///    beats every loose match.
/// 2. A misspelling within a word, up to one edit for short queries and two
///    for long ones. `shatdown` finds `Shutdown`.
/// 3. A subsequence that starts at a word boundary. This is how `vscode` finds
///    `Visual Studio Code`. A subsequence that starts in the middle of a word
///    is rejected, because it matches almost anything: `audio` would otherwise
///    match `Visual Studio Code` on the `u`, `d`, `i`, `o` of `Studio`.
pub fn fuzzy_score(query: &str, haystack: &str) -> Option<i32> {
    let query = truncate(query);
    if query.is_empty() {
        return Some(0);
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let haystack: Vec<char> = haystack.chars().flat_map(char::to_lowercase).collect();

    if let Some(position) = find_substring(&haystack, &query) {
        return Some(substring_score(&query, &haystack, position));
    }

    approximate_score(&query, &haystack).max(subsequence_score(&query, &haystack))
}

fn substring_score(query: &[char], haystack: &[char], position: usize) -> i32 {
    let mut score = 400 + 40 * query.len() as i32;
    if position == 0 {
        score += 300;
    } else if is_boundary(haystack, position) {
        score += 180;
    }
    score - ((haystack.len() - query.len()) as i32 / 2)
}

/// The number of edits allowed by [`approximate_score`]. Short queries get
/// none: a single edit in three letters matches too much.
fn max_typos(length: usize) -> usize {
    match length {
        0..=3 => 0,
        4..=8 => 1,
        _ => 2,
    }
}

fn approximate_score(query: &[char], haystack: &[char]) -> Option<i32> {
    let max_distance = max_typos(query.len());
    if max_distance == 0 || haystack.is_empty() || query.len() > haystack.len() + max_distance {
        return None;
    }

    let min_length = query.len().saturating_sub(max_distance).max(1);
    let max_length = query.len() + max_distance;
    let mut best: Option<i32> = None;

    for start in 0..haystack.len() {
        if start != 0 && !is_boundary(haystack, start) {
            continue;
        }
        if start + min_length > haystack.len() {
            break;
        }
        let end_limit = (start + max_length).min(haystack.len());
        for end in (start + min_length)..=end_limit {
            let distance = edit_distance(query, &haystack[start..end]);
            if distance > max_distance {
                continue;
            }

            let mut score = 320 - distance as i32 * 120;
            score += if start == 0 { 60 } else { 40 };
            score -= start as i32 / 4;
            score -= ((end - start) as i32 - query.len() as i32).abs() * 10;
            best = Some(best.map_or(score, |current| current.max(score)));
        }
    }

    best
}

fn subsequence_score(query: &[char], haystack: &[char]) -> Option<i32> {
    let first = *query.first()?;
    let mut best: Option<i32> = None;

    for start in 0..haystack.len() {
        if haystack[start] != first {
            continue;
        }
        if start != 0 && !is_boundary(haystack, start) {
            continue;
        }
        if let Some(score) = subsequence_score_from(query, haystack, start) {
            best = Some(best.map_or(score, |current| current.max(score)));
        }
    }

    best
}

fn subsequence_score_from(query: &[char], haystack: &[char], start: usize) -> Option<i32> {
    let mut positions = Vec::with_capacity(query.len());
    positions.push(start);

    let mut cursor = start + 1;
    for &character in &query[1..] {
        while cursor < haystack.len() && haystack[cursor] != character {
            cursor += 1;
        }
        if cursor >= haystack.len() {
            return None;
        }
        positions.push(cursor);
        cursor += 1;
    }

    let mut score = if start == 0 { 60 } else { 40 };
    let mut previous: Option<usize> = None;
    for &position in &positions {
        score += 5;
        if position != start && is_boundary(haystack, position) {
            score += 25;
        }
        if let Some(previous) = previous {
            let gap = position - previous - 1;
            if gap == 0 {
                score += 30;
            } else {
                score -= (gap.min(8) as i32) * 4;
            }
        }
        previous = Some(position);
    }

    let span = positions.last().copied().unwrap_or(start) - start + 1;
    score -= ((span - query.len()) as i32) * 2;
    Some(score)
}

fn find_substring(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Optimal string alignment distance: Levenshtein plus adjacent swaps.
fn edit_distance(left: &[char], right: &[char]) -> usize {
    let mut before_previous: Vec<usize> = Vec::new();
    let mut previous: Vec<usize> = (0..=right.len()).collect();

    for (index, &left_character) in left.iter().enumerate() {
        let mut current = vec![index + 1; right.len() + 1];
        for (column, &right_character) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_character != right_character);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);

            if index > 0
                && column > 0
                && left_character == right[column - 1]
                && left[index - 1] == right_character
            {
                current[column + 1] = current[column + 1].min(before_previous[column - 1] + 1);
            }
        }
        before_previous = previous;
        previous = current;
    }

    previous[right.len()]
}

fn truncate(query: &str) -> String {
    query.chars().take(MAX_QUERY_CHARS).collect()
}

fn is_boundary(haystack: &[char], position: usize) -> bool {
    position > 0 && !haystack[position - 1].is_alphanumeric()
}

/// Filters and orders items for a client-search source. An empty query keeps
/// the source order.
pub fn filter_and_sort(items: &[MenuItem], query: &str) -> Vec<MenuItem> {
    filter_and_sort_with(items, query, |_| 0)
}

/// Like [`filter_and_sort`], with a per-item bonus added to every match score.
/// Sources use it to break near-ties with launch history.
pub fn filter_and_sort_with(
    items: &[MenuItem],
    query: &str,
    bonus: impl Fn(&MenuItem) -> i32,
) -> Vec<MenuItem> {
    let query = truncate(query);
    let query = query.trim();
    if query.is_empty() {
        return items.to_vec();
    }

    let mut scored: Vec<(i32, &MenuItem)> = items
        .iter()
        .filter_map(|item| {
            let name = fuzzy_score(query, &item.name).map(|score| score * 2 + 50);
            let keyword = item
                .keywords
                .iter()
                .filter_map(|keyword| fuzzy_score(query, keyword))
                .max()
                .map(|score| score + 20);
            let description = item
                .description
                .as_deref()
                .and_then(|description| fuzzy_score(query, description))
                .map(|score| score + 10);

            name.max(keyword)
                .max(description)
                .map(|score| (score + bonus(item), item))
        })
        .collect();

    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    scored.into_iter().map(|(_, item)| item.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{ItemState, MenuValue};

    fn item(name: &str) -> MenuItem {
        let mut item = MenuItem::new(name.to_lowercase(), name);
        item.state = ItemState::Normal;
        item
    }

    #[test]
    fn exact_and_prefix_matches_score_higher() {
        let exact = fuzzy_score("fire", "Firefox").unwrap();
        let middle = fuzzy_score("fire", "Campfire").unwrap();
        assert!(exact > middle);
    }

    #[test]
    fn subsequences_match() {
        assert!(fuzzy_score("ffx", "Firefox").is_some());
        assert!(fuzzy_score("vscode", "Visual Studio Code").is_some());
        assert!(fuzzy_score("xyz", "Firefox").is_none());
        assert!(fuzzy_score("", "anything").is_some());
    }

    #[test]
    fn typos_are_tolerated() {
        assert!(fuzzy_score("shatdown", "Shutdown").is_some());
        assert!(fuzzy_score("shutdwn", "Shutdown").is_some());
        assert!(fuzzy_score("fierfox", "Firefox").is_some());
        assert!(fuzzy_score("shutdown", "Shutdown").is_some());
    }

    #[test]
    fn scattered_letters_in_the_middle_of_a_word_do_not_match() {
        assert!(fuzzy_score("audio", "Visual Studio Code").is_none());
    }

    #[test]
    fn exact_audio_matches_outrank_the_studio_subsequence() {
        let player = fuzzy_score("audio", "VLC media player (Audio CD)").unwrap();
        let code = fuzzy_score("audio", "Visual Studio Code");
        assert!(code.is_none() || player > code.unwrap());
    }

    #[test]
    fn audio_ranks_audio_apps_first() {
        let mut effects = item("Easy Effects");
        effects.description = Some("Simple audio effects".to_string());
        let items = vec![
            item("Visual Studio Code"),
            item("VLC media player (Audio CD)"),
            effects,
        ];
        let filtered = filter_and_sort(&items, "audio");
        assert_eq!(filtered[0].name, "VLC media player (Audio CD)");
        assert_eq!(filtered[1].name, "Easy Effects");
        assert!(filtered
            .iter()
            .all(|item| item.name != "Visual Studio Code"));
    }

    #[test]
    fn misspelled_queries_find_the_item() {
        let items = vec![item("Protontricks"), item("Shutdown")];
        let filtered = filter_and_sort(&items, "shatdown");
        assert_eq!(filtered[0].name, "Shutdown");
    }

    #[test]
    fn boundary_bonus_applies() {
        let boundary = fuzzy_score("code", "Visual Studio Code").unwrap();
        let inside = fuzzy_score("code", "Encode").unwrap();
        assert!(boundary > inside);
    }

    #[test]
    fn consecutive_characters_score_higher() {
        let consecutive = fuzzy_score("fire", "fire").unwrap();
        let split = fuzzy_score("fire", "f-i-r-e").unwrap();
        assert!(consecutive > split);
    }

    #[test]
    fn empty_query_keeps_source_order() {
        let items = vec![item("Zulu"), item("Alpha")];
        let filtered = filter_and_sort(&items, "");
        assert_eq!(filtered[0].name, "Zulu");
        assert_eq!(filtered[1].name, "Alpha");
    }

    #[test]
    fn results_are_ordered_by_score() {
        let items = vec![item("Firefox"), item("Files"), item("Terminal")];
        let filtered = filter_and_sort(&items, "fil");
        assert_eq!(filtered[0].name, "Files");
        assert!(filtered.iter().all(|item| item.name != "Terminal"));
    }

    #[test]
    fn keywords_and_descriptions_match() {
        let mut editor = item("Editor");
        editor.keywords = vec!["development".to_string()];
        let mut browser = item("Browser");
        browser.description = Some("Browse the development web".to_string());

        let filtered = filter_and_sort(&[editor, browser], "development");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn fields_are_not_searched() {
        let mut item = item("Wallpaper");
        item.fields.insert(
            "path".to_string(),
            MenuValue::String("/secret/path".to_string()),
        );
        let filtered = filter_and_sort(&[item], "secret");
        assert!(filtered.is_empty());
    }

    #[test]
    fn a_bonus_breaks_near_ties() {
        let items = vec![item("Files"), item("File Manager")];
        let plain = filter_and_sort(&items, "file");
        assert_eq!(plain[0].name, "Files");

        let boosted = filter_and_sort_with(&items, "file", |item| {
            if item.name == "File Manager" {
                40
            } else {
                0
            }
        });
        assert_eq!(boosted[0].name, "File Manager");
    }
}
