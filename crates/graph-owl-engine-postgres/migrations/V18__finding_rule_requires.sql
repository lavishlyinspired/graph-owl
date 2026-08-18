-- What a rule needs before it can conclude anything.
--
-- `reconcile_pack` reported `evaluated = rules.len()` and nothing per rule, so
-- a rule that returned no rows because the data it reads is absent was
-- indistinguishable from one that returned no rows because nothing is wrong.
-- On a compliance screen those render identically as "no issues", and they are
-- opposite claims: one is "checked, clean", the other is "never checked".
--
-- Declared rather than inferred from the query text. Inference looks cheaper
-- and is wrong in exactly the case that matters: `payment-overdue.sparql`
-- mentions gst:PaymentEvent inside an OPTIONAL block, so the class being
-- absent does *not* stop the rule concluding — it is how the rule detects a
-- never-paid invoice. A rule author knows which of its inputs are load-bearing;
-- a parser does not.
--
-- Empty array = "needs nothing special", which is every rule that reads only
-- what any reconciliation already has.
ALTER TABLE finding_rules
    ADD COLUMN requires JSONB NOT NULL DEFAULT '[]'::jsonb;
