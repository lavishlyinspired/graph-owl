-- Epic 5 Slice E: what the last validation pass found.
--
-- **A table, not flakes.** Validation results are derived and recomputable, and
-- putting them in the graph would make them subject to their own validation —
-- a shape over `dsc:` predicates would start reporting on the violation records
-- describing violations of it. They are also replaced wholesale every run,
-- which is a delete-and-insert against a table and a retract-and-assert storm
-- against a flake store.
--
-- Stored rather than computed per request, because `GET /validation/report` is
-- read by a queue view that polls: recomputing a full-graph pass on every poll
-- makes the cheapest possible client the most expensive query in the system.
CREATE TABLE validation_results (
    id          UUID PRIMARY KEY,

    -- The logical instant the pass ran against. **Not a wall clock**: the
    -- question a steward asks is "is this report current", and against a graph
    -- whose own history is a logical clock the honest answer is a `t` that can
    -- be compared with the graph's own. A timestamp would only say when the
    -- computer got round to it.
    computed_at_t BIGINT NOT NULL,
    computed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    shape       TEXT NOT NULL,
    focus_node  TEXT NOT NULL,
    -- Null for a combinator, which is about no single predicate.
    path        TEXT,
    constraint_kind TEXT NOT NULL,
    severity    TEXT NOT NULL,
    message     TEXT NOT NULL,
    -- Rendered, because the offending value may be any flake type and the
    -- report only ever displays it. Storing it typed would mean ten nullable
    -- columns to serve one string.
    actual      TEXT,
    -- The suggestion, as the API returns it. Null when no mechanical repair
    -- exists — which is most of them, and saying so is the point.
    suggestion  JSONB
);

-- The queue's own query: by severity, worst first, then stable by node.
CREATE INDEX validation_results_triage
    ON validation_results (severity, focus_node, shape);

-- "What is wrong with this asset" — the panel on every asset page.
CREATE INDEX validation_results_by_node ON validation_results (focus_node);
