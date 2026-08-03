-- Quality signals — Epic 30.
--
-- **graph-owl ingests and displays results; it does not run tests.** That
-- boundary is why there is no scheduler, no assertion language and no executor
-- here: those are a product in their own right, and building them would
-- dominate the roadmap.

-- The reusable template (decision 3a). "Freshness within 24 hours" is **one**
-- definition applied to eight hundred tables, not eight hundred unrelated cases
-- that happen to share a name. Without the split the same check is registered
-- under a thousand names, nothing can be reported on across assets, and
-- changing the threshold means editing a thousand rows.
CREATE TABLE test_definitions (
    id               UUID PRIMARY KEY,
    name             TEXT NOT NULL UNIQUE CHECK (name <> ''),
    -- Free-form: the producing tool names it (`not_null`, `dbt_utils.equality`),
    -- and an enum would mean a release per testing tool.
    test_type        TEXT NOT NULL CHECK (test_type <> ''),
    description      TEXT,
    -- ISO 8601, days and smaller only. A year and a month are not fixed lengths
    -- of time, and "did this run within its cadence" has to be answerable by
    -- subtracting two instants — see `graph_owl_core::quality::parse_cadence`.
    expected_cadence TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A named collection with an owner (decision 3b) — the unit a team is
-- accountable for and the unit a report is produced against.
CREATE TABLE test_suites (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE CHECK (name <> ''),
    -- `RESTRICT`: a suite whose owner vanished is a suite nobody is accountable
    -- for, and Epic 11's principal deletion already knows how to refuse or
    -- reassign.
    owner       TEXT REFERENCES teams (id) ON DELETE RESTRICT,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One definition applied to one target (decision 3).
--
-- **A stable identity results attach to**, so history survives a rename: the
-- case keeps its id while its target's name changes underneath it.
CREATE TABLE test_cases (
    id               UUID PRIMARY KEY,
    name             TEXT NOT NULL CHECK (name <> ''),

    -- Nullable, because a one-off check that belongs to no template is a real
    -- thing — a definition is the way to share a check, not a way to require
    -- ceremony before making one.
    definition_id    UUID REFERENCES test_definitions (id) ON DELETE SET NULL,
    suite_id         UUID REFERENCES test_suites (id) ON DELETE SET NULL,

    -- The asset or column under test. FQN rather than an id for the same reason
    -- every other cross-entity reference since Epic 24 uses one: a column is
    -- addressed that way, and a case on a column is the case that matters most.
    target_fqn       TEXT NOT NULL CHECK (target_fqn <> ''),

    test_type        TEXT NOT NULL CHECK (test_type <> ''),
    description      TEXT,
    -- Overrides the definition's when set, so one table can be checked hourly
    -- while the same definition runs daily elsewhere.
    expected_cadence TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Scoped to the target: `not_null` on two different columns is two cases,
    -- and a globally unique name would forbid the second.
    UNIQUE (target_fqn, name)
);

CREATE INDEX test_cases_by_target ON test_cases (target_fqn);
CREATE INDEX test_cases_by_suite ON test_cases (suite_id) WHERE suite_id IS NOT NULL;
CREATE INDEX test_cases_by_definition
    ON test_cases (definition_id) WHERE definition_id IS NOT NULL;

-- The observation stream (decision 1). A result is a fact at a point in time;
-- current health is *derived* from recent results, because storing only the
-- latest loses the history that makes a signal trustworthy.
CREATE TABLE test_results (
    id          UUID PRIMARY KEY,
    case_id     UUID NOT NULL REFERENCES test_cases (id) ON DELETE CASCADE,
    status      TEXT NOT NULL CHECK (status IN ('success', 'failed', 'aborted')),
    observed_at TIMESTAMPTZ NOT NULL,
    message     TEXT,
    -- Whatever the producing tool measured — row counts, null percentages,
    -- thresholds. Structured so a console can render it, opaque so graph-owl
    -- does not have to model every tool's output.
    metrics     JSONB,

    -- **The dedup key.** A retried push must not double-count: the same check
    -- at the same instant is one observation however many times it arrives.
    UNIQUE (case_id, observed_at)
);

-- Health reads this way: the newest result per case.
CREATE INDEX test_results_latest ON test_results (case_id, observed_at DESC);
-- Pruning reads the other way: everything older than the window.
CREATE INDEX test_results_by_age ON test_results (observed_at);
