-- Lineage and classification, as facts — Epic 6 Slice F.
--
-- Lineage lives in `lineage_edges` and is the source of truth there; these
-- predicates are what let it be *projected* into the graph, which is what makes
-- it reasonable-over. A rule that propagates a classification downstream needs
-- `feeds` as a fact, and so does any SPARQL question about lineage.
--
-- All `many`: a table feeds several tables, is derived from several, and can
-- carry several classifications. Single-valued would make the second assertion
-- silently supersede the first — a lineage graph quietly losing edges is the
-- one thing a lineage graph must never do.
--
-- `propagatesAlong` is the **opt-in** that makes classification propagation
-- safe. Stated on the classification itself rather than in configuration, so
-- "why did this spread" is answerable from the graph. `many` because one
-- marking may legitimately follow more than one kind of edge.
--
-- value_type: 0=ref 1=str
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (1, 'feeds',           0, TRUE, TRUE),
    (1, 'derivedFrom',     0, TRUE, TRUE),
    (1, 'classification',  0, TRUE, TRUE),
    (1, 'propagatesAlong', 0, TRUE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
