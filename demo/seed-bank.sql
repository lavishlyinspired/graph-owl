-- A retail + corporate bank's core estate, Indian context.
-- Chosen over a toy schema because it exercises what a catalog is actually for:
-- PII that must be classified, regulatory tables whose lineage is auditable,
-- and the difference between an asset that is wrong and one that is unreported.
DROP SCHEMA IF EXISTS core_banking CASCADE;
DROP SCHEMA IF EXISTS payments CASCADE;
DROP SCHEMA IF EXISTS lending CASCADE;
DROP SCHEMA IF EXISTS risk CASCADE;
DROP SCHEMA IF EXISTS regulatory CASCADE;

CREATE SCHEMA core_banking;
CREATE SCHEMA payments;
CREATE SCHEMA lending;
CREATE SCHEMA risk;
CREATE SCHEMA regulatory;

-- ---------- core_banking: the customer and account masters ----------
CREATE TABLE core_banking.customers (
    customer_id      BIGINT PRIMARY KEY,
    cif_number       TEXT NOT NULL UNIQUE,
    full_name        TEXT NOT NULL,
    pan              CHAR(10),
    aadhaar_last4    CHAR(4),
    mobile           TEXT NOT NULL,
    email            TEXT,
    date_of_birth    DATE NOT NULL,
    ckyc_number      TEXT,
    risk_category    TEXT NOT NULL,
    onboarded_at     TIMESTAMPTZ NOT NULL,
    branch_ifsc      CHAR(11) NOT NULL
);

CREATE TABLE core_banking.accounts (
    account_id       BIGINT PRIMARY KEY,
    account_number   TEXT NOT NULL UNIQUE,
    customer_id      BIGINT NOT NULL REFERENCES core_banking.customers(customer_id),
    product_code     TEXT NOT NULL,
    ifsc             CHAR(11) NOT NULL,
    currency         CHAR(3) NOT NULL,
    balance          NUMERIC(18,2) NOT NULL,
    status           TEXT NOT NULL,
    opened_on        DATE NOT NULL,
    closed_on        DATE
);

CREATE TABLE core_banking.branches (
    ifsc             CHAR(11) PRIMARY KEY,
    branch_name      TEXT NOT NULL,
    city             TEXT NOT NULL,
    state            TEXT NOT NULL,
    region           TEXT NOT NULL
);

-- ---------- payments: UPI / NEFT / RTGS / IMPS ----------
CREATE TABLE payments.upi_transactions (
    txn_id           TEXT PRIMARY KEY,
    rrn              CHAR(12) NOT NULL,
    payer_vpa        TEXT NOT NULL,
    payee_vpa        TEXT NOT NULL,
    payer_account_id BIGINT REFERENCES core_banking.accounts(account_id),
    amount           NUMERIC(18,2) NOT NULL,
    status           TEXT NOT NULL,
    npci_response    TEXT,
    initiated_at     TIMESTAMPTZ NOT NULL,
    settled_at       TIMESTAMPTZ
);

CREATE TABLE payments.neft_rtgs_transactions (
    utr              CHAR(22) PRIMARY KEY,
    channel          TEXT NOT NULL,
    remitter_account BIGINT REFERENCES core_banking.accounts(account_id),
    beneficiary_ifsc CHAR(11) NOT NULL,
    beneficiary_acct TEXT NOT NULL,
    amount           NUMERIC(18,2) NOT NULL,
    value_date       DATE NOT NULL,
    status           TEXT NOT NULL
);

CREATE TABLE payments.mandates (
    umrn             TEXT PRIMARY KEY,
    account_id       BIGINT REFERENCES core_banking.accounts(account_id),
    max_amount       NUMERIC(18,2) NOT NULL,
    frequency        TEXT NOT NULL,
    valid_till       DATE
);

-- ---------- lending ----------
CREATE TABLE lending.loan_accounts (
    loan_id          BIGINT PRIMARY KEY,
    customer_id      BIGINT NOT NULL REFERENCES core_banking.customers(customer_id),
    product          TEXT NOT NULL,
    sanctioned_amt   NUMERIC(18,2) NOT NULL,
    outstanding      NUMERIC(18,2) NOT NULL,
    roi              NUMERIC(5,2) NOT NULL,
    tenure_months    INT NOT NULL,
    dpd              INT NOT NULL,
    asset_class      TEXT NOT NULL,
    disbursed_on     DATE NOT NULL
);

CREATE TABLE lending.repayments (
    repayment_id     BIGINT PRIMARY KEY,
    loan_id          BIGINT NOT NULL REFERENCES lending.loan_accounts(loan_id),
    due_date         DATE NOT NULL,
    paid_on          DATE,
    emi_amount       NUMERIC(18,2) NOT NULL,
    principal_comp   NUMERIC(18,2),
    interest_comp    NUMERIC(18,2)
);

CREATE TABLE lending.credit_bureau_pulls (
    pull_id          BIGINT PRIMARY KEY,
    customer_id      BIGINT NOT NULL REFERENCES core_banking.customers(customer_id),
    bureau           TEXT NOT NULL,
    score            INT,
    pulled_at        TIMESTAMPTZ NOT NULL
);

-- ---------- risk ----------
CREATE TABLE risk.aml_alerts (
    alert_id         BIGINT PRIMARY KEY,
    customer_id      BIGINT NOT NULL REFERENCES core_banking.customers(customer_id),
    scenario         TEXT NOT NULL,
    severity         TEXT NOT NULL,
    raised_at        TIMESTAMPTZ NOT NULL,
    disposition      TEXT
);

CREATE TABLE risk.fraud_scores (
    txn_id           TEXT PRIMARY KEY,
    model_version    TEXT NOT NULL,
    score            NUMERIC(5,4) NOT NULL,
    decision         TEXT NOT NULL,
    scored_at        TIMESTAMPTZ NOT NULL
);

CREATE VIEW risk.high_value_upi AS
    SELECT t.txn_id, t.amount, t.payer_vpa, f.score
    FROM payments.upi_transactions t
    LEFT JOIN risk.fraud_scores f ON f.txn_id = t.txn_id
    WHERE t.amount >= 100000;

-- ---------- regulatory: the tables an auditor will ask about ----------
CREATE TABLE regulatory.ctr_submissions (
    submission_id    BIGINT PRIMARY KEY,
    reporting_month  DATE NOT NULL,
    customer_id      BIGINT NOT NULL,
    aggregate_amount NUMERIC(18,2) NOT NULL,
    filed_on         DATE,
    fiu_ack          TEXT
);

CREATE TABLE regulatory.crilc_exposures (
    exposure_id      BIGINT PRIMARY KEY,
    borrower_cif     TEXT NOT NULL,
    reporting_qtr    TEXT NOT NULL,
    fund_based       NUMERIC(18,2) NOT NULL,
    non_fund_based   NUMERIC(18,2) NOT NULL,
    sma_class        TEXT
);

CREATE VIEW regulatory.npa_summary AS
    SELECT asset_class, count(*) AS accounts, sum(outstanding) AS exposure
    FROM lending.loan_accounts
    WHERE dpd > 90
    GROUP BY asset_class;
