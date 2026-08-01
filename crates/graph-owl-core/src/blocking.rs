//! Blocking keys — Epic 17 Slice B.
//!
//! Four cheap, over-inclusive fingerprints so a write-time storage adapter
//! can index candidates instead of comparing every pair (pairwise comparison
//! is O(n^2) and unusable past a few thousand entities). A blocking-key
//! collision is never a match by itself — these functions only narrow who
//! gets compared by the real scorer (`graph_owl_resolution::score`), so
//! being occasionally over-inclusive costs a few extra comparisons, never a
//! wrong merge.
//!
//! Pure and I/O-free so a Postgres adapter and any in-memory test double
//! derive identical values from the same four functions, rather than two
//! implementations of "the same key" that can silently drift apart.

const KEY_SEPARATOR: char = '\u{1}';

/// `lower(fqn)`. An asset's `fully_qualified_name` is always already
/// canonical — [`crate::fqn::derive`] and [`crate::fqn::child_of`] reject a
/// segment that contains the separator at write time — so, unlike matching a
/// *draft's* raw external representation, there is no quoting ambiguity left
/// to resolve here; lowercasing is the whole transformation.
#[must_use]
pub fn normalized_fqn_key(fqn: &str) -> String {
    fqn.to_lowercase()
}

/// `lower(name)` plus the parent's FQN, catching the same entity reported
/// under a differently-named (but not differently-scoped) parent. `None` (a
/// root asset) is its own distinct scope, not interchangeable with "no
/// parent recorded" — two roots share this key only with each other, never
/// with a scoped entity of the same name.
#[must_use]
pub fn name_parent_key(name: &str, parent_fqn: Option<&str>) -> String {
    format!(
        "{}{KEY_SEPARATOR}{}",
        name.to_lowercase(),
        parent_fqn.unwrap_or_default().to_lowercase()
    )
}

/// One letter's Soundex digit, or `None` for a vowel (no digit).
fn soundex_digit(c: char) -> Option<char> {
    match c.to_ascii_uppercase() {
        'B' | 'F' | 'P' | 'V' => Some('1'),
        'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => Some('2'),
        'D' | 'T' => Some('3'),
        'L' => Some('4'),
        'M' | 'N' => Some('5'),
        'R' => Some('6'),
        _ => None,
    }
}

/// American Soundex, simplified: `h`/`w` are treated as silent rather than as
/// non-breaking separators between two letters that share a digit (the
/// textbook algorithm's treatment, which disambiguates cases like
/// "Ashcraft" that this function does not attempt to distinguish).
/// Acceptable here because a blocking key only has to be over-inclusive, not
/// exact — the real scorer decides a match, so a code that occasionally
/// clusters one extra name costs a comparison, not a wrong merge.
#[must_use]
pub fn soundex(name: &str) -> String {
    let letters: Vec<char> = name.chars().filter(char::is_ascii_alphabetic).collect();
    let Some(&first) = letters.first() else {
        return String::new();
    };

    let mut result = String::new();
    result.push(first.to_ascii_uppercase());
    let mut last_digit = soundex_digit(first);

    for &c in &letters[1..] {
        if result.len() == 4 {
            break;
        }
        let this_digit = soundex_digit(c);
        if let Some(digit) = this_digit
            && this_digit != last_digit
        {
            result.push(digit);
        }
        last_digit = this_digit;
    }

    while result.len() < 4 {
        result.push('0');
    }
    result
}

/// A fingerprint of a table's column set: lowercased, sorted, comma-joined.
/// Not a cryptographic digest — Postgres indexes arbitrary-length `TEXT`
/// fine, and the joined form stays directly readable in a query plan or a
/// debug log, which a fixed-size hash would trade away for no benefit at
/// this table's scale. Empty input produces the empty string, a fixed key
/// shared by every entity with no known columns — a weak signal on its own,
/// but the other three keys still discriminate.
#[must_use]
pub fn column_hash_key(column_names: &[String]) -> String {
    let mut sorted: Vec<String> = column_names.iter().map(|c| c.to_lowercase()).collect();
    sorted.sort();
    sorted.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_fqn_key_lowercases() {
        assert_eq!(normalized_fqn_key("PROD.Sales.Orders"), "prod.sales.orders");
    }

    #[test]
    fn name_parent_key_combines_both_lowercased() {
        assert_eq!(
            name_parent_key("Orders", Some("Warehouse.Sales")),
            format!("orders{KEY_SEPARATOR}warehouse.sales")
        );
    }

    #[test]
    fn name_parent_key_treats_no_parent_as_its_own_scope() {
        let root = name_parent_key("orders", None);
        let scoped = name_parent_key("orders", Some("warehouse.sales"));
        assert_ne!(root, scoped);
    }

    #[test]
    fn two_roots_with_the_same_name_share_a_key() {
        assert_eq!(
            name_parent_key("orders", None),
            name_parent_key("orders", None)
        );
    }

    #[test]
    fn two_scoped_entities_with_different_parents_do_not_share_a_key() {
        assert_ne!(
            name_parent_key("orders", Some("warehouse.sales")),
            name_parent_key("orders", Some("warehouse.finance"))
        );
    }

    #[test]
    fn soundex_of_empty_name_is_empty() {
        assert_eq!(soundex(""), "");
    }

    #[test]
    fn soundex_matches_the_canonical_robert_rupert_pair() {
        assert_eq!(soundex("Robert"), "R163");
        assert_eq!(soundex("Rupert"), "R163");
    }

    #[test]
    fn soundex_matches_the_canonical_jackson_example() {
        // Exercises the C/G/J/K/Q/S/X/Z group (the collapsed "cks" run) and
        // the M/N group (the trailing 'n') as *visible* digits — not just a
        // difference from some other name, which a deleted match arm could
        // still satisfy by coincidence.
        assert_eq!(soundex("Jackson"), "J250");
    }

    #[test]
    fn soundex_matches_the_canonical_wilson_example() {
        // Exercises the L group as a visible digit: unlike "Lloyd"'s second
        // 'l' (collapsed against the first letter's own code either way),
        // this 'l' sits mid-word with nothing to collapse against.
        assert_eq!(soundex("Wilson"), "W425");
    }

    #[test]
    fn soundex_pads_short_names_with_zeros() {
        assert_eq!(soundex("Lee"), "L000");
    }

    #[test]
    fn soundex_collapses_adjacent_duplicate_codes() {
        // A-b-b-o-t: the second 'b' shares "orders"'s first 'b' digit and is
        // adjacent to it, so it contributes no second digit.
        assert_eq!(soundex("Abbot"), "A130");
    }

    #[test]
    fn soundex_is_case_insensitive() {
        assert_eq!(soundex("robert"), soundex("ROBERT"));
    }

    #[test]
    fn soundex_differs_for_clearly_different_names() {
        assert_ne!(soundex("Robert"), soundex("Zephyr"));
    }

    #[test]
    fn soundex_ignores_non_alphabetic_characters() {
        assert_eq!(soundex("Robert"), soundex("Rob-ert!"));
    }

    #[test]
    fn column_hash_key_ignores_order() {
        let a = vec!["id".to_string(), "amount".to_string()];
        let b = vec!["amount".to_string(), "id".to_string()];
        assert_eq!(column_hash_key(&a), column_hash_key(&b));
    }

    #[test]
    fn column_hash_key_is_case_insensitive() {
        let a = vec!["ID".to_string()];
        let b = vec!["id".to_string()];
        assert_eq!(column_hash_key(&a), column_hash_key(&b));
    }

    #[test]
    fn column_hash_key_differs_for_different_column_sets() {
        let a = vec!["id".to_string()];
        let b = vec!["id".to_string(), "amount".to_string()];
        assert_ne!(column_hash_key(&a), column_hash_key(&b));
    }

    #[test]
    fn empty_column_set_produces_the_empty_key() {
        assert_eq!(column_hash_key(&[]), String::new());
    }
}
