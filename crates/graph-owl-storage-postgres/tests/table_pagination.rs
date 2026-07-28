mod common;

use chrono::Utc;
use graph_owl_core::{
    Table,
    page::{Cursor, PageRequest},
};
use graph_owl_storage::Storage;
use graph_owl_storage_postgres::PostgresStorage;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn table_named(fqn: &str) -> Table {
    let now = Utc::now();
    Table {
        id: Uuid::new_v4(),
        name: fqn.rsplit('.').next().unwrap_or(fqn).to_string(),
        fully_qualified_name: fqn.to_string(),
        description: None,
        created_at: now,
        updated_at: now,
    }
}

async fn seed(storage: &PostgresStorage, count: usize) {
    for n in 0..count {
        storage
            .insert_table(table_named(&format!("warehouse.public.t{n:03}")))
            .await
            .expect("seed insert should succeed");
    }
}

fn request(limit: usize, after: Option<&Cursor>) -> PageRequest {
    PageRequest::new(Some(limit), after.map(Cursor::encode).as_deref())
        .expect("test builds valid requests")
}

#[tokio::test]
async fn pages_are_ordered_and_do_not_overlap() {
    let (storage, _container) = test_storage().await;
    seed(&storage, 30).await;

    let mut seen: Vec<String> = Vec::new();
    let mut after: Option<Cursor> = None;
    let mut pages = 0;

    loop {
        let page = storage
            .list_tables(&request(10, after.as_ref()))
            .await
            .expect("list should succeed");
        pages += 1;
        seen.extend(page.data.iter().map(|t| t.fully_qualified_name.clone()));

        let Some(token) = page.paging.after.as_deref() else {
            break;
        };
        after = Some(Cursor::decode(token).expect("server cursors decode"));
        assert!(pages < 10, "pagination failed to terminate");
    }

    assert_eq!(pages, 3, "30 rows in pages of 10");
    assert_eq!(seen.len(), 30, "every row appears exactly once: {seen:?}");

    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 30, "no duplicates across pages");
    assert_eq!(seen, sorted, "rows arrive in sort order");
}

/// The property that makes this keyset rather than offset pagination: a row
/// inserted before the reader's position shifts every later row under OFFSET,
/// so page 2 either repeats an item from page 1 or skips one entirely.
#[tokio::test]
async fn an_insert_between_pages_neither_skips_nor_duplicates() {
    let (storage, _container) = test_storage().await;
    seed(&storage, 30).await;

    let first = storage
        .list_tables(&request(10, None))
        .await
        .expect("page 1");
    let cursor = Cursor::decode(first.paging.after.as_deref().expect("more pages"))
        .expect("server cursors decode");

    // Sorts before everything already read — the exact case OFFSET gets wrong.
    storage
        .insert_table(table_named("warehouse.public.aaa_inserted"))
        .await
        .expect("insert between pages");

    let second = storage
        .list_tables(&request(10, Some(&cursor)))
        .await
        .expect("page 2");

    let page_one: Vec<&str> = first
        .data
        .iter()
        .map(|t| t.fully_qualified_name.as_str())
        .collect();
    let page_two: Vec<&str> = second
        .data
        .iter()
        .map(|t| t.fully_qualified_name.as_str())
        .collect();

    assert!(
        !page_two.iter().any(|fqn| page_one.contains(fqn)),
        "page 2 must not repeat page 1: {page_one:?} vs {page_two:?}"
    );
    assert_eq!(
        page_two,
        vec![
            "warehouse.public.t010",
            "warehouse.public.t011",
            "warehouse.public.t012",
            "warehouse.public.t013",
            "warehouse.public.t014",
            "warehouse.public.t015",
            "warehouse.public.t016",
            "warehouse.public.t017",
            "warehouse.public.t018",
            "warehouse.public.t019",
        ],
        "page 2 is unaffected by an insert before the cursor"
    );
}

/// The mutator the plan flags: `LIMIT n+1` versus `LIMIT n` is what decides
/// whether `after` is null, and getting it wrong truncates a result set
/// silently — the reader believes it has read everything.
#[tokio::test]
async fn only_the_final_page_has_a_null_cursor() {
    let (storage, _container) = test_storage().await;
    seed(&storage, 20).await;

    let first = storage
        .list_tables(&request(10, None))
        .await
        .expect("page 1");
    assert!(
        first.paging.after.is_some(),
        "a full page with rows behind it must carry a cursor"
    );

    let cursor = Cursor::decode(first.paging.after.as_deref().unwrap()).expect("decodes");
    let second = storage
        .list_tables(&request(10, Some(&cursor)))
        .await
        .expect("page 2");

    assert_eq!(second.data.len(), 10);
    assert!(
        second.paging.after.is_none(),
        "the last page must be signalled by a null cursor, even when it is full"
    );
}

#[tokio::test]
async fn an_empty_table_returns_an_empty_final_page() {
    let (storage, _container) = test_storage().await;

    let page = storage
        .list_tables(&request(10, None))
        .await
        .expect("list should succeed on an empty table");

    assert!(page.data.is_empty());
    assert!(page.paging.after.is_none());
}

#[tokio::test]
async fn a_cursor_past_the_end_returns_an_empty_final_page() {
    let (storage, _container) = test_storage().await;
    seed(&storage, 3).await;

    let beyond = Cursor::new("zzzzz", Uuid::nil());
    let page = storage
        .list_tables(&request(10, Some(&beyond)))
        .await
        .expect("a cursor past the end is not an error");

    assert!(page.data.is_empty());
    assert!(page.paging.after.is_none());
}
