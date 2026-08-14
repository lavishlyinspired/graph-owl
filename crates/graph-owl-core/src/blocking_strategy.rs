//! Domain-neutral blocking strategies — Epic 105 DN-2.
//!
//! [`crate::blocking`] ships four blocking keys, and every one of them is
//! **catalog-shaped**: `normalized_fqn_key` takes a fully-qualified name,
//! `name_parent_key` takes a containment parent, `column_hash_key` takes
//! column names. They are the right keys for deduplicating tables and
//! columns, and they are useless to a domain that has none of those things —
//! a hospitality pack blocking on phone numbers, a clinical pack on
//! practitioner id plus date of birth, an automotive pack on VIN.
//!
//! **The rule that keeps this neutral: a strategy is named after its
//! algorithm, never after a domain, and reads whichever fields the caller
//! names.** `Exact`, `Normalized`, `Phonetic`, `NGram`, `NumericBucket`,
//! `DateWindow` and `Composite` are algorithms. A `GstinKey` variant would be
//! the per-domain hardcoding this module exists to remove — and it would be
//! the same mistake as the three medical namespaces that ended up as Rust
//! constants in `graph-owl-core` (`plans/105-domain-neutrality.md`).
//!
//! A pack therefore configures matching entirely in data:
//!
//! ```yaml
//! # packs/<domain>/matching.yaml
//! blocking:
//!   - strategy: normalized
//!     fields: [taxIdentifier]
//!   - strategy: composite
//!     of:
//!       - { strategy: normalized, fields: [invoiceNumber] }
//!       - { strategy: date_window, fields: [invoiceDate], days: 7 }
//! ```
//!
//! **Over-inclusive on purpose, exactly as `blocking` already is.** A key
//! collision is never a match by itself — these only narrow who gets compared
//! by the real scorer, so being occasionally over-inclusive costs a few extra
//! comparisons and never a wrong merge. Every strategy here inherits that
//! contract, and the `None` cases below (a missing field, an unparsable
//! number) return no key rather than an empty one, because an empty key would
//! block every incomplete record together and make the cheapest stage the
//! most expensive.
//!
//! Pure and I/O-free, like its sibling: the storage adapter and any in-memory
//! double derive identical keys from the same code rather than from two
//! implementations of "the same key" that drift.

use std::collections::BTreeMap;

/// The field values of one record, by field name.
///
/// A `BTreeMap<String, String>` rather than a domain struct precisely because
/// this module must not know what a record *is*. Everything is a string here;
/// interpreting one as a number or a date is a strategy's own job, and its
/// failure to do so is a `None` key, never a panic.
pub type Record = BTreeMap<String, String>;

/// Separates the parts of a composed key.
///
/// `\u{1}` for the same reason [`crate::blocking`] uses it: it cannot occur in
/// a field value that survived validation, so `("ab", "c")` and `("a", "bc")`
/// can never collide into one key.
const KEY_SEPARATOR: char = '\u{1}';

/// How to derive a blocking key from a record.
///
/// Serializable so a pack's `matching.yaml` deserializes straight into it —
/// the configuration *is* the strategy, with no translation step where a
/// domain name could sneak in.
///
/// `PartialEq` but deliberately **not** `Eq`: [`Strategy::NumericBucket`]
/// carries an `f64`, and a bucket width is a quantity a pack writes rather
/// than a token the code compares — pretending it has total equality would be
/// claiming NaN is orderable, which is exactly the value `key` refuses below.
/// **Internally tagged on `strategy`, matching what both shipped packs
/// already write** — `strategy = "date_window"`, its parameters beside it,
/// and a `composite`'s parts under `of` as strategies in their own right.
/// This derive is what the doc comment above has promised since Epic 105
/// DN-2 and did not have: `Strategy` carried no `Deserialize` at all, so
/// every `[[matching.blocking]]` in the tree was configuration nothing could
/// read. Plan 111 Slice D.
///
/// **`deny_unknown_fields` is deliberate.** A pack that writes `window = 7`
/// where the strategy expects `days` has made a mistake that a permissive
/// parser turns into a *silently different key* — the block narrows, the
/// near-misses stop being found, and nothing anywhere reports a problem.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "strategy", rename_all = "snake_case", deny_unknown_fields)]
pub enum Strategy {
    /// The field values verbatim, joined. Case- and whitespace-sensitive.
    ///
    /// For identifiers that are already canonical and where a difference in
    /// case is a genuine difference — a case-sensitive external key, a hash.
    Exact {
        /// Fields to read, in order.
        fields: Vec<String>,
    },
    /// Lowercased, trimmed, inner whitespace collapsed, then joined.
    ///
    /// The workhorse: two systems reporting `"ACME  Ltd "` and `"acme ltd"`
    /// block together, which is the overwhelmingly common shape of the same
    /// entity recorded twice.
    Normalized {
        /// Fields to read, in order.
        fields: Vec<String>,
    },
    /// Soundex over each field, for names spelled by ear.
    ///
    /// Latin-alphabet only by construction — see [`crate::blocking::soundex`].
    /// A pack whose names are not Latin should not choose this rather than
    /// expect it to degrade gracefully, which is why it produces `None` when a
    /// field yields no letters at all instead of a key every such record
    /// shares.
    Phonetic {
        /// Fields to read, in order.
        fields: Vec<String>,
    },
    /// Every `n`-character window of the normalized value.
    ///
    /// **Which behaviour you get depends on which method you call, and the
    /// difference is the whole point of this variant.** [`Strategy::keys`]
    /// indexes a record under *each* window, so two values sharing any window
    /// block together — a transposed identifier (`…1ZM` against `…1MZ`)
    /// leaves most windows intact and is found. [`Strategy::key`] joins the
    /// whole sorted window **set** into one string, which is an exact match on
    /// the set and cannot see a transposition at all.
    ///
    /// The doc comment here used to claim the single key tolerated
    /// transposition. It never did, and the ordering test that appeared to
    /// check compared a record with itself — found by Plan 111 Slice D, the
    /// first time this strategy was run against real data.
    // **`ngram`, not the `n_gram` that `rename_all = "snake_case"` derives.**
    // Both shipped packs already write `strategy = "ngram"`, and the packs are
    // the contract this parser has to meet — renaming the wire name to suit
    // the Rust identifier would silently stop reading configuration that has
    // been on disk since Epic 105. Found by the round-trip test, which is the
    // only place the two spellings ever meet.
    #[serde(rename = "ngram")]
    NGram {
        /// Fields to read, in order.
        fields: Vec<String>,
        /// Window size. `0` yields no key rather than an empty one.
        n: usize,
    },
    /// A number floored into fixed-width buckets.
    ///
    /// Two records whose amounts differ by rounding or a small fee land in one
    /// bucket. **Adjacent buckets do not collide** — a value on a boundary
    /// blocks with only one side, which is why a pack matching on amounts
    /// usually pairs this with another key rather than relying on it alone.
    NumericBucket {
        /// Fields to read, in order.
        fields: Vec<String>,
        /// Bucket width. `0` or negative yields no key.
        width: f64,
    },
    /// A date floored into fixed-width day windows, from the epoch.
    ///
    /// The temporal sibling of [`Strategy::NumericBucket`], and it carries the
    /// same boundary caveat. Expects `YYYY-MM-DD` prefixed values, which is
    /// what every ISO-8601 date and datetime starts with.
    DateWindow {
        /// Fields to read, in order.
        fields: Vec<String>,
        /// Window width in days. `0` yields no key.
        days: i64,
    },
    /// Several strategies joined into one key.
    ///
    /// The composed key is `None` if **any** part is `None`: a composite is a
    /// conjunction, and treating a missing part as empty would silently widen
    /// the block to every record missing that field — the opposite of what the
    /// pack asked for.
    Composite {
        /// The parts, in order.
        of: Vec<Strategy>,
    },
}

/// Lowercase, trim, and collapse internal whitespace runs to one space.
fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Read every named field, or `None` if any is absent or empty.
///
/// All-or-nothing because a key built from a subset of the fields the pack
/// named is a *different* key wearing the same name — it would block records
/// together on weaker evidence than the configuration asked for, invisibly.
fn values<'a>(record: &'a Record, fields: &[String]) -> Option<Vec<&'a str>> {
    if fields.is_empty() {
        return None;
    }
    fields
        .iter()
        .map(|f| record.get(f).map(String::as_str).filter(|v| !v.is_empty()))
        .collect()
}

fn join(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .collect::<Vec<_>>()
        .join(&KEY_SEPARATOR.to_string())
}

/// Days since the Unix epoch for a `YYYY-MM-DD`-prefixed value.
///
/// Hand-rolled rather than pulled from `chrono` because this crate's blocking
/// module is deliberately dependency-free, and the arithmetic needed is a
/// day count, not calendar handling. Uses the standard civil-from-days
/// algorithm, which is exact for every Gregorian date.
fn days_from_epoch(value: &str) -> Option<i64> {
    let date = value.get(..10)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shift + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

impl Strategy {
    /// **Every** key this strategy indexes a record under — Plan 111 Slice D.
    ///
    /// For every strategy but one this is `key` in a vector, and the extra
    /// method would be pointless. [`Strategy::NGram`] is the exception, and
    /// it is the reason this exists: `key` joins a value's *whole* sorted
    /// window set into one string, so any change to any window changes the
    /// key — which means a transposed identifier, the classic data-entry
    /// error `NGram` was added for, never blocks with its own correction.
    /// **The doc comment on that variant claimed otherwise and no test
    /// checked**; the ordering test that looked like it did compared a record
    /// with itself. Found by running the strategy against real data for the
    /// first time.
    ///
    /// Indexing under each window is what n-gram blocking means: two records
    /// sharing any window are worth comparing, and a transposition leaves
    /// most windows intact. Windows are prefixed by field position so two
    /// fields cannot collide into one bucket.
    ///
    /// **A `Composite` keys through `key`, not through this.** A conjunction
    /// over multi-key parts is a cross product, which multiplies rather than
    /// narrows — and narrowing is the entire job of a blocking stage. No
    /// shipped pack composes an n-gram, so this costs nothing today and is
    /// stated rather than discovered later.
    #[must_use]
    pub fn keys(&self, record: &Record) -> Vec<String> {
        let Self::NGram { fields, n } = self else {
            return self.key(record).into_iter().collect();
        };
        if *n == 0 {
            return Vec::new();
        }
        let Some(values) = values(record, fields) else {
            return Vec::new();
        };
        let mut keys: Vec<String> = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
            let chars: Vec<char> = normalize(value).chars().collect();
            if chars.len() < *n {
                // One unkeyable field makes the whole record unkeyable by this
                // strategy — `values`' own all-or-nothing rule, applied here
                // too, so a partial key never masquerades as the configured
                // one.
                return Vec::new();
            }
            for window in chars.windows(*n) {
                let key = format!(
                    "{index}{KEY_SEPARATOR}{}",
                    window.iter().collect::<String>()
                );
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    /// Derive this strategy's key for one record.
    ///
    /// `None` means "this record cannot be blocked by this strategy" — a
    /// missing field, an unparsable value, a degenerate parameter. The caller
    /// indexes nothing rather than indexing everything under one key.
    ///
    /// **One key, so an [`Strategy::NGram`] here is an exact match on the
    /// whole window set** — see [`Strategy::keys`], which is what a caller
    /// wanting n-gram blocking's actual behaviour should use.
    #[must_use]
    pub fn key(&self, record: &Record) -> Option<String> {
        match self {
            Self::Exact { fields } => Some(join(
                values(record, fields)?.into_iter().map(str::to_string),
            )),
            Self::Normalized { fields } => {
                Some(join(values(record, fields)?.into_iter().map(normalize)))
            }
            Self::Phonetic { fields } => {
                let codes: Option<Vec<String>> = values(record, fields)?
                    .into_iter()
                    .map(|v| {
                        let code = crate::blocking::soundex(v);
                        // An empty Soundex means the value had no letters at
                        // all. Keying on "" would block every such record
                        // together, which is the one thing a blocking key
                        // must never do.
                        (!code.is_empty()).then_some(code)
                    })
                    .collect();
                Some(join(codes?))
            }
            Self::NGram { fields, n } => {
                if *n == 0 {
                    return None;
                }
                let grams: Option<Vec<String>> = values(record, fields)?
                    .into_iter()
                    .map(|v| {
                        let chars: Vec<char> = normalize(v).chars().collect();
                        if chars.len() < *n {
                            return None;
                        }
                        let mut windows: Vec<String> = chars
                            .windows(*n)
                            .map(|w| w.iter().collect::<String>())
                            .collect();
                        windows.sort_unstable();
                        windows.dedup();
                        Some(windows.join(""))
                    })
                    .collect();
                Some(join(grams?))
            }
            Self::NumericBucket { fields, width } => {
                // `is_finite` before the sign check, and both matter. A NaN
                // width passes `<= 0.0` (every NaN comparison is false), and
                // an infinite one divides every amount to `0.0` — either way
                // the whole corpus lands under one key, which is the single
                // failure a blocking stage must never have.
                if !width.is_finite() || *width <= 0.0 {
                    return None;
                }
                let buckets: Option<Vec<String>> = values(record, fields)?
                    .into_iter()
                    .map(|v| {
                        let parsed: f64 = v.replace([',', ' '], "").parse().ok()?;
                        // `+ 0.0` normalizes `-0.0` to `0.0`. Without it an
                        // amount of `-0` and one of `0` fall in the same
                        // bucket but format as `-0` and `0` — two keys for
                        // one bucket, which is a *missed* match rather than a
                        // wrong one, and correspondingly harder to notice.
                        let bucket = (parsed / width).floor() + 0.0;
                        // Formatted rather than cast to an integer: `as i64`
                        // saturates instead of failing, so a quotient past
                        // `i64::MAX` would silently become `i64::MAX` and file
                        // every enormous amount under one key. Formatting has
                        // no ceiling, so absurd-but-finite amounts still get
                        // distinct buckets. The `is_finite` guard remains for
                        // the genuinely degenerate case — Rust's own `f64`
                        // parser accepts "NaN" and "inf" as *values*, so a
                        // field carrying either reaches this arithmetic.
                        bucket.is_finite().then(|| format!("{bucket:.0}"))
                    })
                    .collect();
                Some(join(buckets?))
            }
            Self::DateWindow { fields, days } => {
                if *days <= 0 {
                    return None;
                }
                let windows: Option<Vec<String>> = values(record, fields)?
                    .into_iter()
                    .map(|v| {
                        let day = days_from_epoch(v)?;
                        Some(day.div_euclid(*days).to_string())
                    })
                    .collect();
                Some(join(windows?))
            }
            Self::Composite { of } => {
                if of.is_empty() {
                    return None;
                }
                let parts: Option<Vec<String>> = of.iter().map(|s| s.key(record)).collect();
                Some(join(parts?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: &[(&str, &str)]) -> Record {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    // ── the neutrality property itself ──────────────────────────────────

    #[test]
    fn one_strategy_serves_domains_that_share_no_vocabulary() {
        // The claim this module exists to make true: the *same* configured
        // strategy, with different field names, blocks a hotel guest, a
        // clinical practitioner and a vehicle — none of which the code knows
        // anything about.
        let guest = Strategy::Normalized {
            fields: fields(&["guestPhone"]),
        };
        let practitioner = Strategy::Normalized {
            fields: fields(&["npi"]),
        };
        let vehicle = Strategy::Normalized {
            fields: fields(&["vin"]),
        };

        assert_eq!(
            guest.key(&record(&[("guestPhone", " +44 20 7946 0958 ")])),
            Some("+44 20 7946 0958".to_string()),
        );
        assert_eq!(
            practitioner.key(&record(&[("npi", "1234567893")])),
            Some("1234567893".to_string()),
        );
        assert_eq!(
            vehicle.key(&record(&[("vin", "1HGCM82633A004352")])),
            Some("1hgcm82633a004352".to_string()),
        );
    }

    #[test]
    fn a_strategy_reads_only_the_fields_it_was_given() {
        // The negative: a strategy that quietly incorporated other fields
        // would make two records with identical keyed values block apart.
        let s = Strategy::Normalized {
            fields: fields(&["a"]),
        };

        assert_eq!(
            s.key(&record(&[("a", "x"), ("b", "1")])),
            s.key(&record(&[("a", "x"), ("b", "2")])),
        );
    }

    // ── exact vs normalized ─────────────────────────────────────────────

    #[test]
    fn exact_preserves_case_and_normalized_does_not() {
        let value = record(&[("id", "AbC")]);

        assert_eq!(
            Strategy::Exact {
                fields: fields(&["id"])
            }
            .key(&value),
            Some("AbC".to_string()),
        );
        assert_eq!(
            Strategy::Normalized {
                fields: fields(&["id"])
            }
            .key(&value),
            Some("abc".to_string()),
        );
    }

    #[test]
    fn normalized_collapses_whitespace_so_two_spellings_of_one_name_agree() {
        let s = Strategy::Normalized {
            fields: fields(&["name"]),
        };

        assert_eq!(
            s.key(&record(&[("name", "  ACME   Ltd ")])),
            s.key(&record(&[("name", "acme ltd")])),
        );
    }

    #[test]
    fn two_different_names_do_not_share_a_normalized_key() {
        let s = Strategy::Normalized {
            fields: fields(&["name"]),
        };

        assert_ne!(
            s.key(&record(&[("name", "acme")])),
            s.key(&record(&[("name", "acme two")])),
        );
    }

    // ── the separator, and why it is not a plain join ────────────────────

    #[test]
    fn field_boundaries_cannot_be_forged_by_moving_a_character() {
        // `("ab","c")` and `("a","bc")` must not collide. A plain
        // concatenation would make them one key and silently block two
        // unrelated records together.
        let s = Strategy::Exact {
            fields: fields(&["x", "y"]),
        };

        assert_ne!(
            s.key(&record(&[("x", "ab"), ("y", "c")])),
            s.key(&record(&[("x", "a"), ("y", "bc")])),
        );
    }

    // ── missing data yields no key, never a shared one ──────────────────

    #[test]
    fn a_missing_field_yields_no_key_rather_than_an_empty_one() {
        // The single most important negative here: an empty key would block
        // every incomplete record together, turning the cheapest stage into
        // the most expensive one.
        let s = Strategy::Normalized {
            fields: fields(&["a", "b"]),
        };

        assert_eq!(s.key(&record(&[("a", "x")])), None);
    }

    #[test]
    fn an_empty_field_value_counts_as_missing() {
        let s = Strategy::Normalized {
            fields: fields(&["a"]),
        };

        assert_eq!(s.key(&record(&[("a", "")])), None);
    }

    #[test]
    fn a_strategy_naming_no_fields_yields_no_key() {
        assert_eq!(
            Strategy::Exact { fields: vec![] }.key(&record(&[("a", "x")])),
            None,
        );
    }

    // ── phonetic ────────────────────────────────────────────────────────

    #[test]
    fn phonetic_blocks_names_spelled_by_ear_together() {
        let s = Strategy::Phonetic {
            fields: fields(&["surname"]),
        };

        assert_eq!(
            s.key(&record(&[("surname", "Smith")])),
            s.key(&record(&[("surname", "Smyth")])),
        );
    }

    #[test]
    fn phonetic_still_separates_genuinely_different_names() {
        let s = Strategy::Phonetic {
            fields: fields(&["surname"]),
        };

        assert_ne!(
            s.key(&record(&[("surname", "Smith")])),
            s.key(&record(&[("surname", "Kowalski")])),
        );
    }

    #[test]
    fn a_value_with_no_letters_yields_no_phonetic_key() {
        let s = Strategy::Phonetic {
            fields: fields(&["surname"]),
        };

        assert_eq!(s.key(&record(&[("surname", "12345")])), None);
    }

    // ── n-gram ──────────────────────────────────────────────────────────

    #[test]
    fn ngram_blocks_a_transposition_that_exact_and_normalized_miss() {
        // The reason this strategy exists: a transposed identifier shares
        // almost all its n-grams but no exact or normalized key at all.
        let s = Strategy::NGram {
            fields: fields(&["id"]),
            n: 2,
        };
        let straight = record(&[("id", "abcd")]);
        let transposed = record(&[("id", "abdc")]);

        assert_ne!(
            Strategy::Normalized {
                fields: fields(&["id"])
            }
            .key(&straight),
            Strategy::Normalized {
                fields: fields(&["id"])
            }
            .key(&transposed),
            "normalized cannot see through a transposition — that is the gap",
        );

        let a = s.key(&straight).expect("key");
        let b = s.key(&transposed).expect("key");
        let shared = a.chars().filter(|c| b.contains(*c)).count();
        assert!(shared > 0, "and n-grams overlap where normalized does not");
    }

    #[test]
    fn ngram_is_order_insensitive_within_a_value() {
        // Sorted windows, so two values with the same n-gram set agree even
        // when the windows occur in a different order.
        let s = Strategy::NGram {
            fields: fields(&["id"]),
            n: 2,
        };

        // **This assertion used to be `key(x) == key(x)` on one record**, which
        // is true of every function and proved nothing about ordering. Two
        // genuinely different strings with the same 2-gram set is the claim.
        // Sorted *and deduplicated*, so the key is the window **set**: two
        // values with the same set agree however long they are and whatever
        // order the windows fall in.
        assert_eq!(
            s.key(&record(&[("id", "abab")])),
            s.key(&record(&[("id", "ababab")])),
            "both are the 2-gram set ab/ba",
        );
        assert_ne!(
            s.key(&record(&[("id", "abab")])),
            s.key(&record(&[("id", "abdab")])),
            "a different 2-gram set is a different key",
        );
    }

    /// **`key` cannot find a transposition, and the doc comment above said it
    /// could.** The whole-set key changes whenever any window changes, so
    /// `…1ZM` and `…1MZ` — the classic data-entry error `NGram` was added for
    /// — produce different keys and never block together. Plan 111 Slice D
    /// found this by running the strategy against real data for the first
    /// time; the tautological ordering test above is why it survived.
    ///
    /// [`Strategy::keys`] is the fix, and this is the pair of assertions that
    /// pins the difference between the two.
    mod a_near_miss_needs_more_than_one_key {
        use super::*;

        fn transposed() -> (Strategy, Record, Record) {
            (
                Strategy::NGram {
                    fields: fields(&["id"]),
                    n: 3,
                },
                record(&[("id", "27AAACR5055K1ZM")]),
                record(&[("id", "27AAACR5055K1MZ")]),
            )
        }

        #[test]
        fn the_single_whole_set_key_does_not_see_a_transposition() {
            let (strategy, left, right) = transposed();
            assert_ne!(strategy.key(&left), strategy.key(&right));
        }

        #[test]
        fn but_the_two_share_windows_so_keys_brings_them_together() {
            let (strategy, left, right) = transposed();
            let mine = strategy.keys(&left);
            let theirs = strategy.keys(&right);

            assert!(mine.len() > 1, "an n-gram record indexes under each window");
            assert!(
                mine.iter().any(|key| theirs.contains(key)),
                "a transposition leaves most windows intact:\n{mine:?}\n{theirs:?}",
            );
        }

        /// **And two genuinely unrelated identifiers still do not meet.**
        /// Without this, `keys` returning every window would look correct
        /// while blocking the entire estate into one bucket.
        #[test]
        fn two_unrelated_values_share_no_window() {
            let strategy = Strategy::NGram {
                fields: fields(&["id"]),
                n: 3,
            };
            let mine = strategy.keys(&record(&[("id", "27AAACR5055K1ZM")]));
            let theirs = strategy.keys(&record(&[("id", "09XYZPQ1234B2WQ")]));

            assert!(
                !mine.iter().any(|key| theirs.contains(key)),
                "\n{mine:?}\n{theirs:?}",
            );
        }

        /// Every other strategy has exactly one key, and `keys` must not
        /// quietly change what they mean.
        #[test]
        fn a_single_key_strategy_reports_exactly_its_own_key() {
            let strategy = Strategy::Normalized {
                fields: fields(&["name"]),
            };
            let value = record(&[("name", "ACME  Ltd")]);

            assert_eq!(
                strategy.keys(&value),
                vec![strategy.key(&value).expect("a key")],
            );
        }

        /// **The boundary, from both sides.** A value exactly `n` characters
        /// long has exactly one window and must index under it; one character
        /// shorter has none. Written to kill two surviving mutants that moved
        /// the guard from `<` to `==` and `<=` — both of which throw away the
        /// shortest keyable value, and neither of which any other test could
        /// see, because every other fixture is comfortably longer than `n`.
        #[test]
        fn a_value_exactly_n_long_has_one_window_and_a_shorter_one_has_none() {
            let strategy = Strategy::NGram {
                fields: fields(&["id"]),
                n: 3,
            };

            assert_eq!(strategy.keys(&record(&[("id", "abc")])).len(), 1);
            assert!(strategy.keys(&record(&[("id", "ab")])).is_empty());
        }

        /// A record a strategy cannot key at all indexes under nothing —
        /// never under an empty key, which would block every incomplete
        /// record together and make the cheapest stage the most expensive.
        #[test]
        fn an_unkeyable_record_indexes_under_nothing() {
            let strategy = Strategy::Normalized {
                fields: fields(&["name"]),
            };
            assert!(strategy.keys(&record(&[("other", "x")])).is_empty());
        }
    }

    #[test]
    fn a_value_shorter_than_n_yields_no_key() {
        assert_eq!(
            Strategy::NGram {
                fields: fields(&["id"]),
                n: 4,
            }
            .key(&record(&[("id", "ab")])),
            None,
        );
    }

    #[test]
    fn a_value_exactly_n_long_yields_a_key() {
        // The boundary itself. `<` rather than `<=` is the difference between
        // a four-character identifier being blockable and being invisible to
        // this strategy entirely — and the whole corpus of shortest-valid
        // values is exactly where an off-by-one hides.
        assert_eq!(
            Strategy::NGram {
                fields: fields(&["id"]),
                n: 4,
            }
            .key(&record(&[("id", "abcd")])),
            Some("abcd".to_string()),
        );
    }

    #[test]
    fn a_zero_window_yields_no_key() {
        // Guarding the degenerate parameter explicitly: `windows(0)` panics,
        // so this is a crash the configuration could otherwise cause.
        assert_eq!(
            Strategy::NGram {
                fields: fields(&["id"]),
                n: 0,
            }
            .key(&record(&[("id", "abc")])),
            None,
        );
    }

    // ── numeric bucket ──────────────────────────────────────────────────

    #[test]
    fn amounts_within_one_bucket_block_together_and_across_buckets_do_not() {
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        assert_eq!(
            s.key(&record(&[("amount", "1010.00")])),
            s.key(&record(&[("amount", "1099.99")])),
        );
        assert_ne!(
            s.key(&record(&[("amount", "1099.99")])),
            s.key(&record(&[("amount", "1100.01")])),
            "adjacent buckets are distinct — the boundary caveat, asserted",
        );
    }

    #[test]
    fn thousands_separators_do_not_defeat_a_numeric_bucket() {
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        assert_eq!(
            s.key(&record(&[("amount", "1,010.00")])),
            s.key(&record(&[("amount", "1010.00")])),
        );
    }

    #[test]
    fn a_negative_amount_buckets_downward_not_toward_zero() {
        // `floor`, not truncation: truncating would put -0.5 and +0.5 in the
        // same bucket, which is two different amounts blocking together.
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        assert_ne!(
            s.key(&record(&[("amount", "-50")])),
            s.key(&record(&[("amount", "50")])),
        );
    }

    #[test]
    fn an_unparsable_amount_and_a_degenerate_width_both_yield_no_key() {
        assert_eq!(
            Strategy::NumericBucket {
                fields: fields(&["amount"]),
                width: 100.0,
            }
            .key(&record(&[("amount", "not a number")])),
            None,
        );
        assert_eq!(
            Strategy::NumericBucket {
                fields: fields(&["amount"]),
                width: 0.0,
            }
            .key(&record(&[("amount", "10")])),
            None,
        );
    }

    #[test]
    fn a_non_finite_amount_yields_no_key_rather_than_one_shared_bucket() {
        // Rust's own `f64` parser accepts "NaN" and "inf" as *values*, so a
        // field carrying either reaches the arithmetic — and every one of
        // them would floor to a single shared bucket.
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        for amount in ["NaN", "inf", "-inf"] {
            assert_eq!(
                s.key(&record(&[("amount", amount)])),
                None,
                "amount {amount} must not produce a key",
            );
        }
    }

    #[test]
    fn an_absurdly_large_but_finite_amount_still_gets_its_own_bucket() {
        // The reason the bucket is *formatted* rather than cast to an
        // integer: `as i64` saturates instead of failing, so every amount
        // past `i64::MAX` would collapse onto one key. Formatting has no
        // ceiling, so these stay distinct.
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        let huge = s.key(&record(&[("amount", "1e30")]));
        let huger = s.key(&record(&[("amount", "1e300")]));

        assert!(huge.is_some());
        assert_ne!(huge, huger);
    }

    #[test]
    fn negative_zero_and_zero_share_one_bucket_key() {
        // They are the same bucket, and `-0.0` formats as "-0" without the
        // normalization — two keys for one bucket, which is a *missed* match
        // rather than a wrong one and correspondingly harder to notice.
        let s = Strategy::NumericBucket {
            fields: fields(&["amount"]),
            width: 100.0,
        };

        assert_eq!(
            s.key(&record(&[("amount", "-0")])),
            s.key(&record(&[("amount", "0")])),
        );
    }

    #[test]
    fn a_nan_or_infinite_width_yields_no_key_rather_than_one_shared_bucket() {
        // Found while fixing a compile error, not designed for: `f64` is not
        // `Eq`, and chasing that surfaced that NaN passes every `<= 0.0`
        // guard (all NaN comparisons are false) while infinity divides every
        // amount to zero. Either would file the entire corpus under one key —
        // turning the cheapest stage into an O(n^2) comparison of everything.
        for width in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                Strategy::NumericBucket {
                    fields: fields(&["amount"]),
                    width,
                }
                .key(&record(&[("amount", "10")])),
                None,
                "width {width} must not produce a key",
            );
        }
    }

    // ── date window ─────────────────────────────────────────────────────

    #[test]
    fn dates_inside_one_window_block_together() {
        let s = Strategy::DateWindow {
            fields: fields(&["date"]),
            days: 7,
        };

        assert_eq!(
            s.key(&record(&[("date", "2026-08-03")])),
            s.key(&record(&[("date", "2026-08-05")])),
        );
    }

    #[test]
    fn dates_in_different_windows_do_not() {
        let s = Strategy::DateWindow {
            fields: fields(&["date"]),
            days: 1,
        };

        assert_ne!(
            s.key(&record(&[("date", "2026-08-03")])),
            s.key(&record(&[("date", "2026-08-04")])),
        );
    }

    #[test]
    fn a_datetime_is_read_by_its_date_prefix() {
        let s = Strategy::DateWindow {
            fields: fields(&["date"]),
            days: 1,
        };

        assert_eq!(
            s.key(&record(&[("date", "2026-08-03T14:30:00Z")])),
            s.key(&record(&[("date", "2026-08-03")])),
        );
    }

    #[test]
    fn a_date_before_the_epoch_windows_downward() {
        // `div_euclid`, not `/`: integer division truncates toward zero, so
        // 1969-12-31 and 1970-01-01 would share a window under `/`.
        let s = Strategy::DateWindow {
            fields: fields(&["date"]),
            days: 7,
        };

        assert_ne!(
            s.key(&record(&[("date", "1969-12-31")])),
            s.key(&record(&[("date", "1970-01-01")])),
        );
    }

    #[test]
    fn an_unparsable_date_and_a_degenerate_window_both_yield_no_key() {
        assert_eq!(
            Strategy::DateWindow {
                fields: fields(&["date"]),
                days: 7,
            }
            .key(&record(&[("date", "not-a-date")])),
            None,
        );
        assert_eq!(
            Strategy::DateWindow {
                fields: fields(&["date"]),
                days: 7,
            }
            .key(&record(&[("date", "2026-13-01")])),
            None,
            "a month out of range is unparsable, not month 13",
        );
        assert_eq!(
            Strategy::DateWindow {
                fields: fields(&["date"]),
                days: 0,
            }
            .key(&record(&[("date", "2026-08-03")])),
            None,
        );
    }

    #[test]
    fn a_year_before_the_common_era_is_unparsable() {
        // A leading `-` makes the year field of the split empty, so parsing
        // fails before any arithmetic runs. Pinned because it is the input
        // boundary, not because it is exotic.
        assert_eq!(days_from_epoch("-0001-01-01"), None);
    }

    #[test]
    fn january_of_year_zero_crosses_the_era_boundary_correctly() {
        // **The negative-era branch is reachable, and a first attempt at this
        // test missed it.** `days_from_epoch` shifts January and February
        // into the *previous* year before computing the era, so `0000-01-01`
        // computes with year `-1` and takes the `year - 399` path — the one a
        // mutation run flagged. Asserting `is_some()` there passed under both
        // mutations and proved nothing; the exact day number is what
        // distinguishes them.
        assert_eq!(days_from_epoch("0000-01-01"), Some(-719_528));

        // And the shape that makes the number checkable rather than magic:
        // year 0 is a leap year in the proleptic Gregorian calendar (0 mod
        // 400 == 0), so January and February contribute 31 + 29 = 60 days.
        assert_eq!(
            days_from_epoch("0000-03-01").expect("date")
                - days_from_epoch("0000-01-01").expect("date"),
            60,
        );
    }

    #[test]
    fn the_epoch_day_arithmetic_is_right_at_known_points() {
        assert_eq!(days_from_epoch("1970-01-01"), Some(0));
        assert_eq!(days_from_epoch("1970-01-02"), Some(1));
        assert_eq!(days_from_epoch("1969-12-31"), Some(-1));
        // A leap day, and the day after, across a century that is a leap year.
        assert_eq!(
            days_from_epoch("2000-03-01").expect("date")
                - days_from_epoch("2000-02-29").expect("date"),
            1,
        );
        // 1900 is not a leap year; 2000 is. An algorithm that got the
        // century rule wrong would be off by one here.
        assert_eq!(
            days_from_epoch("1900-03-01").expect("date")
                - days_from_epoch("1900-02-28").expect("date"),
            1,
        );
    }

    // ── composite ───────────────────────────────────────────────────────

    #[test]
    fn a_composite_agrees_only_when_every_part_agrees() {
        let s = Strategy::Composite {
            of: vec![
                Strategy::Normalized {
                    fields: fields(&["invoiceNumber"]),
                },
                Strategy::DateWindow {
                    fields: fields(&["invoiceDate"]),
                    days: 7,
                },
            ],
        };
        let base = record(&[("invoiceNumber", "INV-1"), ("invoiceDate", "2026-08-03")]);

        assert_eq!(
            s.key(&base),
            s.key(&record(&[
                ("invoiceNumber", "inv-1"),
                ("invoiceDate", "2026-08-05"),
            ])),
            "same invoice, same week",
        );
        assert_ne!(
            s.key(&base),
            s.key(&record(&[
                ("invoiceNumber", "INV-2"),
                ("invoiceDate", "2026-08-03"),
            ])),
            "a different invoice must not block together on the date alone",
        );
    }

    #[test]
    fn a_composite_with_any_missing_part_yields_no_key() {
        // A conjunction: treating the missing part as empty would widen the
        // block to every record missing that field, which is the opposite of
        // what the configuration asked for.
        let s = Strategy::Composite {
            of: vec![
                Strategy::Normalized {
                    fields: fields(&["a"]),
                },
                Strategy::Normalized {
                    fields: fields(&["b"]),
                },
            ],
        };

        assert_eq!(s.key(&record(&[("a", "x")])), None);
    }

    #[test]
    fn an_empty_composite_yields_no_key() {
        assert_eq!(Strategy::Composite { of: vec![] }.key(&record(&[])), None);
    }

    #[test]
    fn composites_nest() {
        let s = Strategy::Composite {
            of: vec![Strategy::Composite {
                of: vec![Strategy::Normalized {
                    fields: fields(&["a"]),
                }],
            }],
        };

        assert!(s.key(&record(&[("a", "x")])).is_some());
    }

    // ── the generalization is real, not additive ────────────────────────

    #[test]
    fn the_shipped_catalog_keys_are_expressible_as_configurations() {
        // The proof that this generalizes `blocking` rather than sitting
        // beside it: `normalized_fqn_key` is `Normalized` over one field, and
        // `name_parent_key` is `Normalized` over two. If these disagreed, the
        // new module would be a second way to do the same thing — which is
        // the outcome this test exists to prevent.
        let fqn = "prod.db.orders";
        assert_eq!(
            Strategy::Normalized {
                fields: fields(&["fqn"]),
            }
            .key(&record(&[("fqn", fqn)])),
            Some(crate::blocking::normalized_fqn_key(fqn)),
        );

        let expected = crate::blocking::name_parent_key("Orders", Some("prod.db"));
        assert_eq!(
            Strategy::Normalized {
                fields: fields(&["name", "parent"]),
            }
            .key(&record(&[("name", "Orders"), ("parent", "prod.db")])),
            Some(expected),
        );
    }

    /// **The module's own doc comment has promised this since Epic 105 DN-2 —
    /// "the configuration *is* the strategy, with no translation step where a
    /// domain name could sneak in" — and `Strategy` derived no `Deserialize`
    /// at all.** Both shipped packs declare `[[matching.blocking]]` in exactly
    /// this shape and nothing in the workspace has ever parsed one.
    ///
    /// The translation step the doc comment warns about is the point: a
    /// hand-written `match` from a strategy name to a variant is where
    /// somebody adds a domain-shaped special case, so there must not be one.
    mod a_pack_declares_its_strategies_in_data {
        use super::*;

        #[test]
        fn every_strategy_a_shipped_pack_declares_round_trips_from_its_own_toml() {
            #[derive(serde::Deserialize)]
            struct Matching {
                blocking: Vec<Strategy>,
            }

            // Copied in shape, not in content, from `packs/gst/pack.toml` and
            // `packs/hospitality/pack.toml` — every strategy either declares
            // today, so a parser that handles only the simple ones fails here
            // rather than in production on the second pack.
            let declared: Matching = toml::from_str(
                r#"
[[blocking]]
strategy = "normalized"
fields = ["ns:partyId", "ns:documentNumber"]

[[blocking]]
strategy = "phonetic"
fields = ["ns:surname"]

[[blocking]]
strategy = "ngram"
fields = ["ns:partyId"]
n = 3

[[blocking]]
strategy = "composite"

[[blocking.of]]
strategy = "normalized"
fields = ["ns:partyId"]

[[blocking.of]]
strategy = "date_window"
fields = ["ns:documentDate"]
days = 7
"#,
            )
            .expect("a pack's own blocking declaration must parse");

            assert_eq!(declared.blocking.len(), 4);
            assert_eq!(
                declared.blocking[0],
                Strategy::Normalized {
                    fields: fields(&["ns:partyId", "ns:documentNumber"]),
                },
            );
            assert_eq!(
                declared.blocking[2],
                Strategy::NGram {
                    fields: fields(&["ns:partyId"]),
                    n: 3,
                },
            );
            // The composite is the one worth asserting whole: its parts are
            // themselves strategies, so a tagging scheme that works at the top
            // level and not inside `of` would pass the three above.
            assert_eq!(
                declared.blocking[3],
                Strategy::Composite {
                    of: vec![
                        Strategy::Normalized {
                            fields: fields(&["ns:partyId"]),
                        },
                        Strategy::DateWindow {
                            fields: fields(&["ns:documentDate"]),
                            days: 7,
                        },
                    ],
                },
            );
        }

        /// **A strategy name this crate does not implement is a refusal, not a
        /// default.** Silently falling back to `Exact` would give a pack a key
        /// far narrower than it asked for and report nothing wrong — the
        /// blocking stage would simply stop finding the near-misses it was
        /// configured to find.
        #[test]
        fn an_unknown_strategy_name_fails_to_parse_rather_than_defaulting() {
            #[derive(serde::Deserialize)]
            struct One {
                blocking: Vec<Strategy>,
            }
            assert!(
                toml::from_str::<One>(
                    "[[blocking]]\nstrategy = \"telepathy\"\nfields = [\"ns:x\"]\n"
                )
                .is_err(),
            );
        }

        /// A declaration missing the parameter its algorithm needs is the same
        /// class of mistake, caught the same way.
        #[test]
        fn a_strategy_missing_its_own_parameter_fails_to_parse() {
            #[derive(serde::Deserialize)]
            struct One {
                blocking: Vec<Strategy>,
            }
            assert!(
                toml::from_str::<One>("[[blocking]]\nstrategy = \"ngram\"\nfields = [\"ns:x\"]\n")
                    .is_err(),
                "`ngram` without `n` has no window size to use",
            );
        }
    }
}
