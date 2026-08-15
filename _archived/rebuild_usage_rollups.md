# `rebuild_usage_rollups` — removed 16 August 2026

**Verdict**: DEAD. `plans/119-architecture-audit.md` §6.1. Removed from
four live files rather than left in place, because a required `Storage`
trait method with two real implementations is not free to carry — every
future implementor of the port (and there will be more storage backends)
would have had to implement a method nothing calls.

## What it was

A `Storage` trait method plus two adapter implementations plus a `Catalog`
facade wrapper, all four now deleted:

- `crates/graph-owl-storage/src/lib.rs` — the trait method declaration
- `crates/graph-owl-storage-memory/src/lib.rs` — the in-memory adapter's
  implementation
- `crates/graph-owl-storage-postgres/src/lib.rs` — the Postgres adapter's
  implementation
- `crates/graph-owl-api/src/lib.rs` — the `Catalog` facade's pass-through
  wrapper

## Original doc comment (the trait method)

```rust
/// Rebuild an asset's rollups from its raw observations.
///
/// **Exists to be compared against the incremental path**, which is the
/// only way to know that path is correct — Slice B's equivalence test. Not
/// a repair tool: after pruning the raw rows are gone and a rebuild would
/// produce *less* than the truth, which is why nothing calls it in
/// production.
///
/// # Errors
/// [`StorageError::Unexpected`] if the read or write fails.
async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError>;
```

## Why it's dead rather than merely uncalled

The doc comment is explicit that it's *meant* to be uncalled in
production — that alone wasn't the finding. What made it dead: it names its
entire reason to exist as "Slice B's equivalence test," and that test does
not exist anywhere in the tree — not as a `#[test]`/`#[tokio::test]`
function, not named in any `plans/*.md`. A capability whose only stated
consumer never got written has no consumer at all. Independently
re-verified with a fresh grep before removal (§6.1): exactly 5 matches for
`rebuild_usage_rollups` workspace-wide before this change — the four
definitions plus the facade's own one-line internal call into the trait
method — and zero call sites outside those four files.

## The two implementations, for the record

**In-memory** (`graph-owl-storage-memory`) — derived the count on read, so
by construction it could never actually disagree with the incremental path:

```rust
async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError> {
    // Derived on read here, so a rebuild is by construction identical
    // to the incremental answer — which is exactly what the equivalence
    // test asserts of the *real* adapter, where they are two paths.
    Ok(i64::try_from(self.usage_rollups(asset_fqn).await?.len()).unwrap_or(i64::MAX))
}
```

**Postgres** (`graph-owl-storage-postgres`) — a real transaction, delete
then re-derive from raw observations:

```rust
async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError> {
    let mut tx = self
        .pool
        .begin()
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

    sqlx::query("DELETE FROM usage_rollups WHERE asset_fqn = $1")
        .bind(asset_fqn)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let rebuilt = sqlx::query(
        "INSERT INTO usage_rollups (asset_fqn, consumer_key, day, operation, count, total_rows)
         SELECT asset_fqn, consumer_key, occurred_at::date, operation,
                COUNT(*), NULLIF(SUM(coalesce(row_count, 0)), 0)
           FROM usage_observations WHERE asset_fqn = $1
          GROUP BY asset_fqn, consumer_key, occurred_at::date, operation",
    )
    .bind(asset_fqn)
    .execute(&mut *tx)
    .await
    .map_err(|e| StorageError::Unexpected(e.to_string()))?
    .rows_affected();

    tx.commit()
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    Ok(i64::try_from(rebuilt).unwrap_or(i64::MAX))
}
```

## If this needs to come back

The `Storage` trait's own equivalence property (incremental rollups vs. a
from-scratch rebuild agree) is a real, useful thing to test — it just was
never wired up. Re-adding this is legitimate if that test is written
alongside it this time: a property test or integration test that populates
raw observations, computes rollups incrementally, calls
`rebuild_usage_rollups`, and asserts the two agree. Without that test, this
is the same dead weight it was before removal.

The Postgres implementation's SQL (delete + re-derive-and-insert from
`usage_observations`, grouped by `asset_fqn, consumer_key, day, operation`)
is real and correct-looking; if the capability returns, this is a reasonable
starting point rather than something to re-derive from scratch.
