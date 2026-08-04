-- Epic 41 Slice E: a memory can be retracted without being replaced.
--
-- `supersede_memory` already covers *correction* — a memory with a wrong
-- confidence or content is replaced by a better one, and both sides remain
-- readable. Retraction is a different fact: the memory is no longer believed
-- at all, and there may be nothing to replace it with. Modelling it as a
-- no-op supersession would need a placeholder replacement memory for every
-- retraction, which is a memory about nothing.
--
-- Never a delete, matching every other retraction in this schema (the flake
-- model's own `op = false`): the record of what was once believed is most of
-- the value of keeping a record at all.
ALTER TABLE memories
    ADD COLUMN retracted_at TIMESTAMPTZ,
    ADD COLUMN retraction_reason TEXT;

-- Both or neither. A `retracted_at` with no reason is a retraction nobody
-- can act on — the same requirement this project already holds a validation
-- waiver to.
ALTER TABLE memories ADD CONSTRAINT memories_retraction_shape CHECK (
    (retracted_at IS NULL AND retraction_reason IS NULL)
    OR
    (retracted_at IS NOT NULL AND retraction_reason IS NOT NULL AND retraction_reason <> '')
);
