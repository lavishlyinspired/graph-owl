-- Plan 122b B0: durable, multi-client, multi-period workflow state,
-- replacing the SESSION/AI_JOBS module-level dicts. Every workflow table
-- below carries client_id and, where meaningful, period_id — the isolation
-- boundary B0's own AC and RED both test directly.
--
-- No GST noun here on purpose (00d/122b §1's "no GST noun may enter the
-- Rust API" is a Rust-API rule, but the same domain-neutrality reasoning
-- applies to this workflow schema too): "case", "follow_up", "approval"
-- are generic workflow nouns, not gst:Invoice or gst:Supplier — those
-- identities live in GraphOWL, referenced here only by invoice_no/supplier
-- strings a case needs to display itself, never modelled as foreign
-- entities this database owns.

CREATE TABLE client (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    gstin TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE period (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    month TEXT NOT NULL,
    year INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_id, month, year)
);

CREATE TABLE app_user (
    -- Named app_user, not user: "user" is a reserved word in every SQL
    -- dialect and quoting it everywhere it appears is a standing footgun
    -- for no benefit — the mockup's own nav item is "Users", not a fixed
    -- wire-facing noun this needs to match verbatim.
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'preparer',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE case_record (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    invoice_no TEXT NOT NULL,
    reason_code TEXT,
    status TEXT NOT NULL DEFAULT 'open',
    assigned_to UUID REFERENCES app_user(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_case_client_period ON case_record (client_id, period_id);

CREATE TABLE ims_decision (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    case_id UUID REFERENCES case_record(id) ON DELETE CASCADE,
    decision TEXT NOT NULL,
    decided_by UUID REFERENCES app_user(id),
    decided_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_ims_decision_client_period ON ims_decision (client_id, period_id);

CREATE TABLE follow_up (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    case_id UUID REFERENCES case_record(id) ON DELETE CASCADE,
    supplier_name TEXT,
    status TEXT NOT NULL DEFAULT 'drafted',
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at TIMESTAMPTZ
);
CREATE INDEX idx_follow_up_client_period ON follow_up (client_id, period_id);

CREATE TABLE approval (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    decision_type TEXT NOT NULL,
    amount NUMERIC,
    requested_by UUID REFERENCES app_user(id),
    status TEXT NOT NULL DEFAULT 'pending',
    decided_by UUID REFERENCES app_user(id),
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_approval_client_period ON approval (client_id, period_id);

CREATE TABLE note (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID REFERENCES period(id) ON DELETE CASCADE,
    case_id UUID REFERENCES case_record(id) ON DELETE CASCADE,
    author UUID REFERENCES app_user(id),
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_note_client_period ON note (client_id, period_id);

CREATE TABLE deliverable (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'drafted',
    content TEXT,
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_deliverable_client_period ON deliverable (client_id, period_id);

CREATE TABLE mapping_template (
    -- Keyed by client, not client+period: "next month starts already
    -- mapped" (122b §2.3 / the mockup's own Mappings copy) means a mapping
    -- template outlives the period it was first confirmed on.
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    dataset_kind TEXT NOT NULL,
    mapping JSONB NOT NULL,
    tolerance DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_id, dataset_kind)
);
