//! Bolt's connection handshake: the magic preamble and version negotiation
//! that happen before any PackStream-framed message exists.
//!
//! Deliberately not versioned itself — negotiation determines *which*
//! version's message set and structure shapes apply to everything after it,
//! so it has to be readable without knowing that answer yet.

/// The four bytes that identify a connection as speaking Bolt at all. Fixed
/// by the spec; a connection sending anything else is not this protocol and
/// gets no response, only closure.
pub const MAGIC: [u8; 4] = [0x60, 0x60, 0xB0, 0x17];

/// "No protocol version" — the server's answer when none of the client's
/// four offers is one it supports, and the value used to pad an offer list
/// shorter than four.
pub const NO_VERSION: [u8; 4] = [0x00, 0x00, 0x00, 0x00];

/// Encode a single `major.minor` version the way a server's reply (and the
/// simple, pre-4.3 client offer) uses it: a reserved byte, a zero range,
/// then minor, then major, big-endian. A server's reply is always this
/// exact shape — the range byte only ever appears in a *client offer*, per
/// the handshake spec's `4.3` section.
#[must_use]
pub fn encode_version(major: u8, minor: u8) -> [u8; 4] {
    [0x00, 0x00, minor, major]
}

/// Decode a four-byte, range-free version entry back to `(major, minor)` —
/// the shape a server's own reply always has.
#[must_use]
pub fn decode_version(bytes: [u8; 4]) -> (u8, u8) {
    (bytes[3], bytes[2])
}

/// One entry from a client's offer block, decoded in full.
///
/// A **range**, not a single version, since Bolt 4.3 — `minor` is the
/// highest version offered and `range` says how many consecutive minors
/// below it are *also* offered, so this one 4-byte slot can carry several
/// versions of one major at once. The pre-4.3 single-version form
/// [`encode_version`] produces is the special case `range == 0`: an offer
/// containing exactly one version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Offer {
    major: u8,
    minor: u8,
    range: u8,
}

fn decode_offer(bytes: [u8; 4]) -> Offer {
    Offer {
        major: bytes[3],
        minor: bytes[2],
        range: bytes[1],
    }
}

/// Whether `offer` covers `(major, minor)` — same major, and `minor` within
/// `[offer.minor - offer.range, offer.minor]`.
///
/// `minor <= offer.minor` is checked **before** the subtraction below it, so
/// the range check never underflows a `u8` for a version above the offer's
/// ceiling — it simply is not contained, which is correct.
fn offer_contains(offer: Offer, major: u8, minor: u8) -> bool {
    offer.major == major && minor <= offer.minor && offer.minor - minor <= offer.range
}

/// Pick the version to speak, from the client's four offers.
///
/// **First *offer slot* wins**, per spec: a server must assume the offers
/// are in the client's order of preference, so this returns on the first
/// slot that contains anything supported, not on searching every slot for
/// the numerically highest match — the client already decided that
/// ordering by which slot it put a version in. Within one matching slot,
/// the highest version this server supports is chosen — the best it can
/// offer inside the range the client already agreed to accept.
///
/// `offers` is the raw 16-byte block the client sent (four 4-byte entries);
/// `[0,0,0,0]` entries (used to pad an offer list shorter than four) never
/// match anything, since [`NO_VERSION`] is not a version this or any server
/// supports.
///
/// Returns `None` when no offer is supported — the caller replies with
/// [`NO_VERSION`] and closes the connection.
#[must_use]
pub fn negotiate(offers: [[u8; 4]; 4], supported: &[(u8, u8)]) -> Option<(u8, u8)> {
    for raw in offers {
        if raw == NO_VERSION {
            continue;
        }
        let offer = decode_offer(raw);
        let best = supported
            .iter()
            .copied()
            .filter(|&(major, minor)| offer_contains(offer, major, minor))
            .max_by_key(|&(_, minor)| minor);
        if let Some(version) = best {
            return Some(version);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_round_trips_through_encode_and_decode() {
        assert_eq!(decode_version(encode_version(5, 0)), (5, 0));
        assert_eq!(decode_version(encode_version(4, 3)), (4, 3));
    }

    #[test]
    fn encoding_matches_the_spec_example_for_version_4_1() {
        // "Example with version 4.1: `00 00 01 04`" — the spec's own worked
        // example, so this is the boundary case a byte-order slip would fail.
        assert_eq!(encode_version(4, 1), [0x00, 0x00, 0x01, 0x04]);
    }

    #[test]
    fn the_first_supported_offer_in_client_preference_order_is_selected() {
        // Client prefers 5.1, then 5.0; server supports both — 5.1 must win,
        // not 5.0, even though a naive "any match" search could return either.
        let offers = [
            encode_version(5, 1),
            encode_version(5, 0),
            NO_VERSION,
            NO_VERSION,
        ];
        assert_eq!(negotiate(offers, &[(5, 0), (5, 1)]), Some((5, 1)));
    }

    #[test]
    fn a_later_offer_is_selected_when_earlier_ones_are_unsupported() {
        let offers = [
            encode_version(9, 9),
            encode_version(5, 0),
            NO_VERSION,
            NO_VERSION,
        ];
        assert_eq!(negotiate(offers, &[(5, 0)]), Some((5, 0)));
    }

    #[test]
    fn four_genuine_offers_none_supported_is_refused() {
        let offers = [
            encode_version(9, 9),
            encode_version(8, 8),
            encode_version(7, 7),
            encode_version(6, 6),
        ];
        assert_eq!(negotiate(offers, &[(5, 0)]), None);
    }

    #[test]
    fn an_all_zero_offer_list_is_refused_not_matched() {
        // A client sending nothing but padding must not accidentally match
        // NO_VERSION if it were ever (wrongly) added to `supported`.
        assert_eq!(negotiate([NO_VERSION; 4], &[(0, 0)]), None);
    }

    #[test]
    fn padding_entries_are_skipped_rather_than_ending_the_search() {
        // A client that only knows one version pads the rest with
        // NO_VERSION; a supported real offer after the padding must still be
        // found rather than the search stopping at the first zero.
        let offers = [NO_VERSION, encode_version(5, 0), NO_VERSION, NO_VERSION];
        assert_eq!(negotiate(offers, &[(5, 0)]), Some((5, 0)));
    }

    // ---- The 4.3+ ranged-offer form ----
    //
    // A real driver has far more than four versions to offer (this project's
    // own Slice F test saw one list eleven deep) and compresses that into
    // the same four 4-byte slots by offering *ranges* of minors — this is
    // not a hypothetical shape, it is what every current official driver
    // actually sends. Getting this wrong is invisible to every test above,
    // because they only ever construct `range == 0` offers via
    // [`encode_version`].

    /// The spec's own worked example: "versions 4.3 plus two previous minor
    /// versions, 4.2 and 4.1" is encoded `00 02 03 04` — range 2, minor 3,
    /// major 4, covering 4.1 through 4.3.
    fn ranged_offer(major: u8, minor: u8, range: u8) -> [u8; 4] {
        [0x00, range, minor, major]
    }

    #[test]
    fn a_ranged_offer_matches_a_supported_version_inside_the_range() {
        // Offers 5.0 through 5.8 in one slot; this server supports only 5.0,
        // which sits at the *bottom* of the range, not the value actually
        // encoded in the offer's own minor byte (8).
        let offers = [ranged_offer(5, 8, 8), NO_VERSION, NO_VERSION, NO_VERSION];
        assert_eq!(negotiate(offers, &[(5, 0)]), Some((5, 0)));
    }

    #[test]
    fn a_ranged_offer_does_not_match_a_version_below_its_floor() {
        // Range 3 off a ceiling of 8 covers 5.5-5.8, not 5.0.
        let offers = [ranged_offer(5, 8, 3), NO_VERSION, NO_VERSION, NO_VERSION];
        assert_eq!(negotiate(offers, &[(5, 0)]), None);
    }

    #[test]
    fn a_ranged_offer_never_matches_a_different_major() {
        let offers = [ranged_offer(4, 8, 8), NO_VERSION, NO_VERSION, NO_VERSION];
        assert_eq!(negotiate(offers, &[(5, 0)]), None);
    }

    #[test]
    fn the_highest_supported_version_inside_a_matched_range_is_chosen() {
        let offers = [ranged_offer(5, 8, 8), NO_VERSION, NO_VERSION, NO_VERSION];
        assert_eq!(negotiate(offers, &[(5, 0), (5, 4)]), Some((5, 4)));
    }

    #[test]
    fn a_real_driver_offer_list_negotiates_this_servers_supported_version() {
        // Approximates what a current official driver actually sends: 6.0
        // alone, then 4.4 down through 4.2 as a range, then 5.8 down through
        // 5.0 as a range, then 3.0 alone — four slots, eleven versions.
        let offers = [
            encode_version(6, 0),
            ranged_offer(4, 4, 2),
            ranged_offer(5, 8, 8),
            encode_version(3, 0),
        ];
        assert_eq!(negotiate(offers, &[(5, 0)]), Some((5, 0)));
    }
}
