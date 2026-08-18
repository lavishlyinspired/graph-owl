-- The rule's own statement of what it looks for.
--
-- The panel listed rule labels and a provision. `gst:PaymentOverdue` means
-- something to whoever wrote the rule; it means nothing to the reviewer being
-- asked to act on it. The pack already carries a one-line summary — "credit
-- taken on an invoice not paid within 180 days of its date" — and it was
-- simply never carried through the execution record to the screen.
ALTER TABLE rule_outcome ADD COLUMN summary TEXT;
