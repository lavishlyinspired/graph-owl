//! Cursor pagination.
//!
//! Offset pagination drifts: a row inserted before the reader's position
//! shifts every later row by one, so page 2 re-shows an item from page 1 or
//! skips one entirely. Keyset pagination asks "give me what sorts after
//! *this exact row*", which is stable under concurrent insert and does not
//! degrade as the offset grows.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_LIMIT: usize = 25;
pub const MAX_LIMIT: usize = 1000;

/// A decoded position in a sorted result set: the sort key plus the id that
/// breaks ties. Both are needed — a sort key alone is not unique, and two rows
/// sharing one would make the cursor ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub sort_key: String,
    pub id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// Not valid base64, or not the expected `key\0uuid` shape.
    Malformed,
}

impl Cursor {
    #[must_use]
    pub fn new(sort_key: impl Into<String>, id: Uuid) -> Self {
        Self {
            sort_key: sort_key.into(),
            id,
        }
    }

    /// Encodes to an opaque token. Opaque is the point: a client that parses a
    /// cursor becomes coupled to the sort implementation, and every later change
    /// to sort order silently breaks it.
    #[must_use]
    pub fn encode(&self) -> String {
        use base64::Engine as _;
        // NUL separates because it cannot appear in a Postgres text value,
        // so a sort key containing any printable separator still round-trips.
        let raw = format!("{}\0{}", self.sort_key, self.id);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    }

    /// # Errors
    ///
    /// Returns [`CursorError::Malformed`] for anything that is not a token this
    /// codec produced. Never panics — a hand-crafted cursor is a client error,
    /// not a server crash.
    pub fn decode(token: &str) -> Result<Self, CursorError> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| CursorError::Malformed)?;
        let text = String::from_utf8(bytes).map_err(|_| CursorError::Malformed)?;
        let (sort_key, id) = text.split_once('\0').ok_or(CursorError::Malformed)?;
        let id = id.parse::<Uuid>().map_err(|_| CursorError::Malformed)?;
        Ok(Self {
            sort_key: sort_key.to_string(),
            id,
        })
    }
}

/// A validated page request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    pub limit: usize,
    pub after: Option<Cursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRequestError {
    /// `limit` exceeded [`MAX_LIMIT`]. Rejected rather than clamped: silently
    /// returning 1000 of a requested 5000 makes a client believe it has read
    /// everything when it has not.
    LimitTooLarge {
        requested: usize,
        max: usize,
    },
    /// `limit` was zero — a page of nothing is never what a caller means.
    LimitZero,
    MalformedCursor,
}

impl PageRequest {
    /// # Errors
    ///
    /// Returns [`PageRequestError`] for an out-of-range limit or an
    /// undecodable cursor.
    pub fn new(limit: Option<usize>, after: Option<&str>) -> Result<Self, PageRequestError> {
        let limit = limit.unwrap_or(DEFAULT_LIMIT);
        if limit == 0 {
            return Err(PageRequestError::LimitZero);
        }
        if limit > MAX_LIMIT {
            return Err(PageRequestError::LimitTooLarge {
                requested: limit,
                max: MAX_LIMIT,
            });
        }
        let after = after
            .map(Cursor::decode)
            .transpose()
            .map_err(|_| PageRequestError::MalformedCursor)?;
        Ok(Self { limit, after })
    }
}

/// One page of results plus the token for the next.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub data: Vec<T>,
    pub paging: Paging,
}

#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paging {
    /// Token for the next page, or `None` on the last one. Nullness is the
    /// only end-of-results signal a client gets, so it must be exact.
    pub after: Option<String>,
}

impl<T> Page<T> {
    /// Builds a page from `limit + 1` fetched rows.
    ///
    /// The extra row is how "is there a next page" is answered without a second
    /// count query: if it came back, there is more; it is then dropped and the
    /// cursor is taken from the **last returned** row, not the extra one.
    #[must_use]
    pub fn from_overfetch(
        mut rows: Vec<T>,
        limit: usize,
        cursor_of: impl Fn(&T) -> Cursor,
    ) -> Self {
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let after = if has_more {
            rows.last().map(|row| cursor_of(row).encode())
        } else {
            None
        };
        Self {
            data: rows,
            paging: Paging { after },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn a_cursor_round_trips() {
        let cursor = Cursor::new("warehouse.public.orders", uuid(7));
        assert_eq!(Cursor::decode(&cursor.encode()), Ok(cursor));
    }

    #[test]
    fn a_sort_key_containing_punctuation_round_trips() {
        // Dots, colons and slashes all appear in real FQNs; a separator that
        // collides with them would truncate the key on decode.
        let cursor = Cursor::new("a.b:c/d-e_f g", uuid(1));
        assert_eq!(Cursor::decode(&cursor.encode()), Ok(cursor));
    }

    #[test]
    fn the_encoded_cursor_is_not_plainly_readable() {
        let cursor = Cursor::new("warehouse.public.orders", uuid(7));
        assert!(
            !cursor.encode().contains("warehouse"),
            "an opaque token must not leak the sort key verbatim"
        );
    }

    #[test]
    fn a_handcrafted_cursor_is_an_error_not_a_panic() {
        assert_eq!(Cursor::decode("not-base64!!!"), Err(CursorError::Malformed));
    }

    #[test]
    fn a_truncated_cursor_is_an_error_not_a_panic() {
        let encoded = Cursor::new("warehouse.public.orders", uuid(7)).encode();
        let truncated = &encoded[..encoded.len() / 2];
        assert_eq!(Cursor::decode(truncated), Err(CursorError::Malformed));
    }

    #[test]
    fn a_cursor_without_a_separator_is_an_error() {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("no-separator-here");
        assert_eq!(Cursor::decode(&raw), Err(CursorError::Malformed));
    }

    #[test]
    fn a_cursor_with_a_non_uuid_tail_is_an_error() {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("key\0not-a-uuid");
        assert_eq!(Cursor::decode(&raw), Err(CursorError::Malformed));
    }

    #[test]
    fn an_absent_limit_defaults() {
        let request = PageRequest::new(None, None).expect("valid");
        assert_eq!(request.limit, DEFAULT_LIMIT);
        assert_eq!(request.after, None);
    }

    #[test]
    fn the_maximum_limit_is_accepted_and_one_more_is_not() {
        assert!(PageRequest::new(Some(MAX_LIMIT), None).is_ok());
        assert_eq!(
            PageRequest::new(Some(MAX_LIMIT + 1), None),
            Err(PageRequestError::LimitTooLarge {
                requested: MAX_LIMIT + 1,
                max: MAX_LIMIT
            }),
            "the boundary is inclusive; rejecting MAX_LIMIT itself is off by one"
        );
    }

    #[test]
    fn a_zero_limit_is_rejected() {
        assert_eq!(
            PageRequest::new(Some(0), None),
            Err(PageRequestError::LimitZero)
        );
    }

    #[test]
    fn a_malformed_cursor_is_rejected_by_the_request() {
        assert_eq!(
            PageRequest::new(None, Some("!!!")),
            Err(PageRequestError::MalformedCursor)
        );
    }

    #[test]
    fn a_full_page_with_more_behind_it_carries_a_cursor() {
        let rows: Vec<u32> = (0..=3).collect(); // limit 3, overfetched 4
        let page = Page::from_overfetch(rows, 3, |n| {
            Cursor::new(n.to_string(), uuid(u128::from(*n)))
        });
        assert_eq!(page.data, vec![0, 1, 2], "the extra row is dropped");
        assert_eq!(
            page.paging.after,
            Some(Cursor::new("2", uuid(2)).encode()),
            "the cursor comes from the last *returned* row, not the overfetched one"
        );
    }

    #[test]
    fn the_last_page_carries_no_cursor() {
        let rows: Vec<u32> = (0..3).collect(); // limit 3, exactly 3 back
        let page = Page::from_overfetch(rows, 3, |n| {
            Cursor::new(n.to_string(), uuid(u128::from(*n)))
        });
        assert_eq!(page.data, vec![0, 1, 2]);
        assert_eq!(
            page.paging.after, None,
            "nullness is the only end-of-results signal a client gets"
        );
    }

    #[test]
    fn an_empty_result_is_a_last_page_not_an_error() {
        let page = Page::from_overfetch(Vec::<u32>::new(), 3, |n| {
            Cursor::new(n.to_string(), uuid(0))
        });
        assert!(page.data.is_empty());
        assert_eq!(page.paging.after, None);
    }
}
