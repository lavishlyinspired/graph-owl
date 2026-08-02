-- Epic 18 Slice E: per-endpoint rate limiting.
--
-- Nullable: `NULL` means unlimited. `01-api-conventions.md` treats rate
-- limiting as an ingress concern except for per-principal quotas — a
-- registered endpoint's own configured budget is that quota, set by whoever
-- knows this specific sender's expected volume, not a single global number
-- this crate would otherwise have to invent.
ALTER TABLE webhook_endpoints ADD COLUMN rate_limit_per_minute INTEGER;
