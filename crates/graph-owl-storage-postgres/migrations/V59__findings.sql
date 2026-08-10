-- Findings — Epic 105 P5, the platform plan's §6.
--
-- **One table for every domain**, because a GST mismatch, a duplicate guest
-- and an overdue filing are the same thing structurally: something a rule
-- concluded, about a subject, on evidence, under an authority. A per-pack
-- table would make the review queue a per-pack screen, which is the whole
-- thing the pack mechanism exists to avoid.
--
-- **`subject` is TEXT, not a foreign key to `assets`, and that is the design
-- rather than a shortcut.** A pack's subjects are graph subjects — an invoice,
-- a guest, a statutory section — with no asset row by design
-- (`plans/105-domain-neutrality.md` DN-3). A foreign key here would force
-- every domain entity to become a catalog asset, which is exactly the widening
-- Epic 33 already refused.
CREATE TABLE findings (
    id          UUID PRIMARY KEY,

    -- Which pack's rules concluded it. Provenance, and the filter a console
    -- queue scopes by.
    pack        TEXT NOT NULL,
    -- The pack's own vocabulary: `gst:MissingInGstr2b`, `hosp:DuplicateGuest`.
    label       TEXT NOT NULL,
    -- The graph subject, in `Sid`'s `{namespace}:{id}` wire form.
    subject     TEXT NOT NULL,
    summary     TEXT NOT NULL,

    -- **NOT NULL is the invariant, expressed where it cannot be forgotten.**
    -- A finding that cannot be traced to a rule is an accusation rather than a
    -- finding: a reviewer has no way to judge it, only to believe it. The
    -- application refuses one too, and both refusing is the point — a check
    -- that lives only in application code is a check the next writer skips.
    governed_by TEXT NOT NULL CHECK (governed_by <> ''),

    -- The facts behind it, as {subject, predicate, value} triples so a
    -- reviewer can follow each one back into the graph rather than read
    -- somebody's summary of it. Non-empty for the same reason `governed_by`
    -- is non-null.
    evidence    JSONB NOT NULL CHECK (jsonb_array_length(evidence) > 0),

    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'accepted', 'rejected')),
    detected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at  TIMESTAMPTZ,
    decided_by  TEXT,
    -- Required on rejection, the same rule Epic 17's merge queue enforces:
    -- the next run must be able to tell "considered and dismissed" from "not
    -- yet seen".
    reason      TEXT,

    CHECK (status <> 'rejected' OR reason IS NOT NULL)
);

-- **Idempotence, and it is partial on purpose.** Re-running a reconciliation
-- over an unchanged corpus must not double the queue while a reviewer is
-- working in it — but a finding that was decided and then *recurs* is a new
-- instance of the problem, not the same one still open, and deserves its own
-- row. The same shape `resolution_queue` (V23) and `drift_reports` (V51)
-- already use.
CREATE UNIQUE INDEX findings_pending_unique
    ON findings (pack, label, subject)
    WHERE status = 'pending';

-- The review queue's own read: pending findings for one pack, newest first.
CREATE INDEX findings_by_pack_status ON findings (pack, status, detected_at DESC);
