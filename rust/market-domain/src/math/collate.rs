//! String order as the views' `localeCompare` sorts it, so a native sort keeps
//! the tie order the page had. `localeCompare` without a locale is the CLDR
//! root collation in every engine and locale the site renders (English): over
//! ASCII, the whitespace controls, space, then punctuation and symbols in the
//! order below, digits, then letters compared without case first, lowercase
//! before uppercase only when nothing else differs, and the other controls
//! ignored. `tests/fixtures/collation/ascii.json` pins this against the JS
//! engines. Snapshot slugs, stamps and categories are ASCII; anything else
//! sorts after the letters by code point, which is not what ICU does.

use std::cmp::Ordering;

const ORDER: &[u8] = b"\t\n\x0b\x0c\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789";

/// The primary and tertiary weight of a character; `None` is ignorable.
fn weights(c: char) -> Option<(u32, u8)> {
    let letters = ORDER.len() as u32;
    if !c.is_ascii() {
        return Some((letters + 26 + u32::from(c), 0));
    }
    let b = c as u8;
    if b.is_ascii_alphabetic() {
        let primary = letters + u32::from(b.to_ascii_lowercase() - b'a');
        return Some((primary, u8::from(b.is_ascii_uppercase())));
    }
    ORDER.iter().position(|&o| o == b).map(|at| (at as u32, 0))
}

/// A stable sort compiled once: each `sort_by` closure is a copy of the
/// sort in the module every surface downloads, so the view rules sort here.
pub fn sorted<T>(rows: Vec<T>, cmp: &dyn Fn(&T, &T) -> Ordering) -> Vec<T> {
    let mut at: Vec<usize> = (0..rows.len()).collect();
    sort_positions(&mut at, &|a, b| match (rows.get(a), rows.get(b)) {
        (Some(a), Some(b)) => cmp(a, b),
        _ => Ordering::Equal,
    });
    let mut slots: Vec<Option<T>> = rows.into_iter().map(Some).collect();
    at.into_iter()
        .filter_map(|i| slots.get_mut(i).and_then(Option::take))
        .collect()
}

fn sort_positions(at: &mut [usize], cmp: &dyn Fn(usize, usize) -> Ordering) {
    at.sort_by(|a, b| cmp(*a, *b));
}

/// `a.localeCompare(b)`, as an ordering.
pub fn locale_cmp(a: &str, b: &str) -> Ordering {
    let key = |s: &str| s.chars().filter_map(weights).collect::<Vec<_>>();
    let (a, b) = (key(a), key(b));
    a.iter()
        .map(|w| w.0)
        .cmp(b.iter().map(|w| w.0))
        .then_with(|| a.iter().map(|w| w.1).cmp(b.iter().map(|w| w.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_the_fixture_as_the_js_engines_do() {
        let sorted: Vec<String> = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/collation/ascii.json"
        ))
        .unwrap();
        let mut shuffled = sorted.clone();
        shuffled.reverse();
        shuffled.sort_by(|a, b| locale_cmp(a, b));
        assert_eq!(shuffled, sorted);
        for pair in sorted.windows(2) {
            assert_eq!(locale_cmp(&pair[0], &pair[1]), Ordering::Less, "{pair:?}");
        }
    }

    #[test]
    fn ignores_controls_and_falls_back_to_case() {
        assert_eq!(locale_cmp("a\u{1}b", "ab"), Ordering::Equal);
        assert_eq!(locale_cmp("ab", "aB"), Ordering::Less);
        assert_eq!(locale_cmp("aB", "Ab"), Ordering::Less);
        assert_eq!(locale_cmp("B", "a"), Ordering::Greater);
    }
}
