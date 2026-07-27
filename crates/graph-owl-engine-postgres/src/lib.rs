//! Postgres adapter for the graph engine: the flakes table, its four index
//! orderings, and current-state resolution over them.
//!
//! Postgres maintains the four orderings inline on write, which removes an
//! entire indexer subsystem from this design. What it does not remove is
//! choosing which ordering a given pattern should use — that judgement lives
//! here, not in the callers.

pub mod value;

use async_trait::async_trait;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern};
use graph_owl_engine::{EngineError, TripleStore, reject_unset_namespaces};
use sqlx::{PgPool, QueryBuilder, Row, postgres::PgRow};

mod embedded {
    refinery::embed_migrations!("migrations");
}

/// This adapter migrates the same database as the storage adapter but owns a
/// different set of tables. Sharing refinery's default history table would
/// make each runner treat the other's migrations as unknown and refuse to run.
const MIGRATION_TABLE: &str = "refinery_schema_history_engine";

/// The columns a flake is rebuilt from. One constant so the `SELECT` list and
/// the decoder cannot drift apart.
const FLAKE_COLUMNS: &str = "namespace_s, sid_s, namespace_p, sid_p, \
     value_type, value_ref_ns, value_ref_id, value_str, value_bool, value_int, \
     value_float, value_inst, value_json, value_bytes, value_uuid, \
     cx_namespace, cx_id, t, op";

/// The fact identity: everything except `t` and `op`.
///
/// Current-state resolution groups by this and keeps the newest row, so a
/// retraction supersedes the assertion it names.
const FACT_IDENTITY: &str =
    "namespace_s, sid_s, namespace_p, sid_p, value_type, value_key, cx_namespace, cx_id";

/// Columns bound per flake by the insert. Counted from the `INSERT` column
/// list below; the two must move together.
const COLUMNS_PER_FLAKE: usize = 20;

/// Postgres carries the parameter count in the wire protocol as an `int16`, so
/// a single statement can bind at most 65535 values. Not a tuning knob — it is
/// the protocol's own ceiling.
const MAX_BIND_PARAMETERS: usize = 65535;

/// The most flakes one `INSERT` can carry. Derived, not chosen: exceeding it
/// is not slow but a hard driver error, and a projection of a wide table
/// crosses it easily.
const MAX_FLAKES_PER_STATEMENT: usize = MAX_BIND_PARAMETERS / COLUMNS_PER_FLAKE;

pub struct PostgresTripleStore {
    pool: PgPool,
}

impl PostgresTripleStore {
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the connection or migrations fail.
    pub async fn connect(connection_string: &str) -> Result<Self, EngineError> {
        let pool = PgPool::connect(connection_string)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;

        let (mut migration_client, connection) =
            tokio_postgres::connect(connection_string, tokio_postgres::NoTls)
                .await
                .map_err(|e| EngineError::Backend(e.to_string()))?;
        tokio::spawn(connection);

        let mut runner = embedded::migrations::runner();
        runner.set_migration_table_name(MIGRATION_TABLE);
        runner
            .run_async(&mut migration_client)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;

        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Appends the pattern's bound terms as `AND` clauses.
    ///
    /// Every unbound term contributes nothing, which is what lets one method
    /// serve all combinations of bound and unbound positions.
    fn push_pattern_filters<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        pattern: &'a TriplePattern,
    ) {
        if let Some(s) = &pattern.s {
            builder.push(" AND namespace_s = ");
            builder.push_bind(i32::from(s.namespace_code));
            builder.push(" AND sid_s = ");
            builder.push_bind(&s.id);
        }
        if let Some(p) = &pattern.p {
            builder.push(" AND namespace_p = ");
            builder.push_bind(i32::from(p.namespace_code));
            builder.push(" AND sid_p = ");
            builder.push_bind(&p.id);
        }
        if let Some(o) = &pattern.o {
            // Matched on the discriminant plus the encoded key rather than on
            // the typed column, so one clause serves all ten value shapes and
            // the POST index -- which leads with exactly this pair after the
            // predicate -- applies regardless of which shape was asked for.
            builder.push(" AND value_type = ");
            builder.push_bind(o.value_type());
            builder.push(" AND value_key = ");
            builder.push_bind(value::value_key(o));

            // Redundant with value_key, but it is what makes the partial OPST
            // index applicable: without a predicate on the columns that index
            // leads with, the planner cannot use it.
            if let FlakeValue::Ref(reference) = o {
                builder.push(" AND value_ref_ns = ");
                builder.push_bind(i32::from(reference.namespace_code));
                builder.push(" AND value_ref_id = ");
                builder.push_bind(&reference.id);
            }
        }
        match &pattern.cx {
            None => {}
            Some(None) => {
                builder.push(" AND cx_namespace IS NULL");
            }
            Some(Some(cx)) => {
                builder.push(" AND cx_namespace = ");
                builder.push_bind(i32::from(cx.namespace_code));
                builder.push(" AND cx_id = ");
                builder.push_bind(&cx.id);
            }
        }
        if let Some(as_of) = pattern.as_of {
            // Inclusive: as-of exactly a transaction's t must return that
            // transaction's state, not the state before it.
            builder.push(" AND t <= ");
            builder.push_bind(as_of);
        }
    }

    /// The current state of every fact matching the pattern.
    ///
    /// `DISTINCT ON` keeps the newest row per fact; `op` is filtered in the
    /// **outer** query. Filtering it inside the inner `WHERE` is the bug this
    /// design is most prone to: it would exclude the retraction row, so the
    /// superseded assertion underneath would resurface as current.
    fn current_state_query<'a>(
        pattern: &'a TriplePattern,
        select: &str,
        prefix: &str,
    ) -> QueryBuilder<'a, sqlx::Postgres> {
        let mut builder = QueryBuilder::new(prefix);
        builder.push("SELECT ");
        builder.push(select);
        builder.push(" FROM (SELECT DISTINCT ON (");
        builder.push(FACT_IDENTITY);
        builder.push(") ");
        builder.push(FLAKE_COLUMNS);
        builder.push(" FROM flakes WHERE TRUE");
        Self::push_pattern_filters(&mut builder, pattern);
        builder.push(" ORDER BY ");
        builder.push(FACT_IDENTITY);
        builder.push(", t DESC) latest WHERE op");
        builder
    }

    /// The query plan Postgres would use for this pattern, as plain text.
    ///
    /// Deliberately built by [`current_state_query`] rather than by a
    /// reconstruction: explaining a query that merely resembles the one that
    /// runs would confirm nothing. A missing or unused index degrades silently
    /// here — the results stay correct and only the plan changes — so the plan
    /// is the only thing that can catch it.
    ///
    /// [`current_state_query`]: Self::current_state_query
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the plan cannot be produced.
    pub async fn explain(&self, pattern: &TriplePattern) -> Result<String, EngineError> {
        let mut builder = Self::current_state_query(pattern, FLAKE_COLUMNS, "EXPLAIN ");
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String, _>(0))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The one place a flake row is written.
    ///
    /// `op` comes from the calling verb, never from the flake: an assertion
    /// and a retraction differ only in that flag, and letting the caller's
    /// struct decide it would make `retract_flakes(&[some_assertion])` write
    /// an assertion — silently doubling the fact instead of withdrawing it.
    async fn write(&self, flakes: &[Flake], op: bool) -> Result<(), EngineError> {
        reject_unset_namespaces(flakes)?;
        if flakes.is_empty() {
            return Ok(());
        }
        // Encoded up front so a malformed value fails before any row is
        // written, rather than leaving a partial batch behind.
        let encoded = flakes
            .iter()
            .map(|flake| value::columns(&flake.o).map(|columns| (flake, columns)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(EngineError::Backend)?;

        // One statement per chunk, and one transaction for the whole batch.
        //
        // A round trip per flake would make projection the slowest part of
        // every write — a wide table is hundreds of flakes. But a single
        // statement cannot hold an unbounded batch either: past
        // MAX_FLAKES_PER_STATEMENT the driver refuses outright. Chunking is
        // therefore not an optimization, it is what makes a large projection
        // possible at all.
        //
        // The transaction is what keeps the promise the chunking would
        // otherwise break: callers get all the flakes or none of them, so a
        // failure midway cannot leave a half-projected entity that reconciles
        // to something no version of the entity ever was.
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;

        for chunk in encoded.chunks(MAX_FLAKES_PER_STATEMENT) {
            let mut builder = QueryBuilder::new(
                "INSERT INTO flakes (namespace_s, sid_s, namespace_p, sid_p, value_type, \
                 value_key, value_ref_ns, value_ref_id, value_str, value_bool, value_int, \
                 value_float, value_inst, value_json, value_bytes, value_uuid, cx_namespace, \
                 cx_id, t, op) ",
            );
            builder.push_values(chunk, |mut row, (flake, columns)| {
                row.push_bind(i32::from(flake.s.namespace_code))
                    .push_bind(&flake.s.id)
                    .push_bind(i32::from(flake.p.namespace_code))
                    .push_bind(&flake.p.id)
                    .push_bind(columns.value_type)
                    .push_bind(columns.key.clone())
                    .push_bind(columns.ref_ns)
                    .push_bind(columns.ref_id)
                    .push_bind(columns.str_value)
                    .push_bind(columns.bool_value)
                    .push_bind(columns.int_value)
                    .push_bind(columns.float_value)
                    .push_bind(columns.instant_at)
                    .push_bind(columns.json_value.clone())
                    .push_bind(columns.bytes_value)
                    .push_bind(columns.uuid_value)
                    .push_bind(flake.cx.as_ref().map(|cx| i32::from(cx.namespace_code)))
                    .push_bind(flake.cx.as_ref().map(|cx| cx.id.clone()))
                    .push_bind(flake.t)
                    .push_bind(op);
            });
            // Re-asserting an identical fact at the same t converges rather
            // than duplicating, so a retried projection is safe to run again.
            builder.push(" ON CONFLICT DO NOTHING");

            builder
                .build()
                .execute(&mut *transaction)
                .await
                .map_err(|e| EngineError::Backend(e.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;
        Ok(())
    }
}

fn flake_from_row(row: &PgRow) -> Result<Flake, EngineError> {
    let object = value::from_columns(
        row.get("value_type"),
        row.get("value_ref_ns"),
        row.get("value_ref_id"),
        row.get("value_str"),
        row.get("value_bool"),
        row.get("value_int"),
        row.get("value_float"),
        row.get("value_inst"),
        row.get("value_json"),
        row.get("value_bytes"),
        row.get("value_uuid"),
    )
    .map_err(EngineError::Backend)?;

    let namespace = |column: &str| -> Result<u16, EngineError> {
        let raw: i32 = row.get(column);
        u16::try_from(raw)
            .map_err(|_| EngineError::Backend(format!("{column} = {raw} is outside u16")))
    };

    let cx_namespace: Option<i32> = row.get("cx_namespace");
    let cx = match cx_namespace {
        None => None,
        Some(raw) => {
            let ns = u16::try_from(raw).map_err(|_| {
                EngineError::Backend(format!("cx_namespace = {raw} is outside u16"))
            })?;
            Some(Sid::new(ns, row.get::<String, _>("cx_id")))
        }
    };

    Ok(Flake {
        s: Sid::new(namespace("namespace_s")?, row.get::<String, _>("sid_s")),
        p: Sid::new(namespace("namespace_p")?, row.get::<String, _>("sid_p")),
        o: object,
        cx,
        t: row.get("t"),
        op: row.get("op"),
    })
}

#[async_trait]
impl TripleStore for PostgresTripleStore {
    async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
        self.write(flakes, true).await
    }

    async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
        self.write(flakes, false).await
    }

    async fn query_pattern(&self, pattern: &TriplePattern) -> Result<Vec<Flake>, EngineError> {
        let mut builder = Self::current_state_query(pattern, FLAKE_COLUMNS, "");
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;
        rows.iter().map(flake_from_row).collect()
    }

    async fn count(&self, pattern: &TriplePattern) -> Result<u64, EngineError> {
        // The same subquery as query_pattern, counted rather than decoded.
        // Sharing the builder is the point: a count computed by a separate
        // path is a count that can disagree with the rows, and the
        // disagreement always surfaces far away from here.
        let mut builder = Self::current_state_query(pattern, "COUNT(*)", "");
        let row = builder
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(|e| EngineError::Backend(e.to_string()))?;
        let count: i64 = row.get(0);
        u64::try_from(count).map_err(|_| EngineError::Backend(format!("negative count {count}")))
    }

    async fn next_time(&self) -> Result<i64, EngineError> {
        // A single UPDATE ... RETURNING is atomic on its own row, so
        // concurrent callers serialize on it without an explicit lock and
        // neither can observe the other's t.
        //
        // The CTE writes the wall clock in the same statement, so a `t` can
        // never exist without the instant it happened at. Two statements would
        // leave a window in which a crash produces a transaction time that no
        // as-of query can ever resolve to.
        let row = sqlx::query(
            "WITH advanced AS (
                 UPDATE graph_clock SET t = t + 1 WHERE only_row RETURNING t
             ), recorded AS (
                 INSERT INTO graph_transactions (t) SELECT t FROM advanced RETURNING t
             )
             SELECT t FROM recorded",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EngineError::Backend(e.to_string()))?;
        Ok(row.get("t"))
    }

    async fn time_at(&self, at: chrono::DateTime<chrono::Utc>) -> Result<Option<i64>, EngineError> {
        // <= not <: as-of exactly a transaction's instant must include that
        // transaction, or "the state right after the migration" is
        // unaskable.
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT t FROM graph_transactions WHERE at <= $1 ORDER BY at DESC, t DESC LIMIT 1",
        )
        .bind(at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EngineError::Backend(e.to_string()))?;
        Ok(row.map(|(t,)| t))
    }
}

#[cfg(test)]
mod chunking_tests {
    use super::{COLUMNS_PER_FLAKE, MAX_BIND_PARAMETERS, MAX_FLAKES_PER_STATEMENT};

    /// Every property here is decidable at compile time, so all three are
    /// `const` blocks: a violation should fail the build rather than wait for
    /// someone to run the tests, and there is no input that could make one of
    /// them true on one run and false on the next.
    #[test]
    fn the_chunk_size_is_the_largest_one_statement_can_carry() {
        // Fits. Past the ceiling the driver does not slow down, it refuses.
        const { assert!(MAX_FLAKES_PER_STATEMENT * COLUMNS_PER_FLAKE <= MAX_BIND_PARAMETERS) };

        // And is maximal. Without this, any smaller divisor is still correct —
        // every flake lands, in one transaction, every behavioural test passes
        // — while a wide table's projection silently costs hundreds of round
        // trips instead of one. Correct-but-pathological is exactly the failure
        // only a property like this catches.
        const { assert!((MAX_FLAKES_PER_STATEMENT + 1) * COLUMNS_PER_FLAKE > MAX_BIND_PARAMETERS) };

        // And is big enough to matter: a thousand-flake projection is one wide
        // table, and that is the promise `assert_flakes` documents.
        const { assert!(MAX_FLAKES_PER_STATEMENT >= 1_000) };
    }
}
