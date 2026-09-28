-- Roundtable ledger v1 (backend plan section 4.2)

CREATE TABLE panels (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  json TEXT NOT NULL,
  prompt_rubric_version TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  state TEXT NOT NULL,
  stage TEXT NOT NULL,
  reason_code TEXT,
  reason_text TEXT,
  question TEXT NOT NULL,
  reference_text TEXT,
  panel_snapshot_json TEXT NOT NULL,
  limits_json TEXT NOT NULL,
  price_catalog_version TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  app_version TEXT NOT NULL,
  adapter_versions_json TEXT NOT NULL,
  seed TEXT NOT NULL,
  is_sample INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  deadline_at TEXT,
  finished_at TEXT
);
CREATE INDEX runs_created_at ON runs(created_at DESC);

CREATE TABLE seats (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  seat_index INTEGER NOT NULL,
  label TEXT NOT NULL,
  provider TEXT NOT NULL,
  credential_ref TEXT NOT NULL,
  model_id TEXT NOT NULL,
  model_returned_id TEXT,
  settings_json TEXT NOT NULL,
  prompt_answer TEXT NOT NULL,
  prompt_review TEXT NOT NULL,
  prompt_improve TEXT NOT NULL,
  prompt_digests_json TEXT NOT NULL,
  PRIMARY KEY (run_id, seat_index)
);

CREATE TABLE logical_calls (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  stage TEXT NOT NULL,
  reviewer_seat INTEGER NOT NULL,
  target_seat INTEGER,
  rotation INTEGER,
  state TEXT NOT NULL,
  attempts_used INTEGER NOT NULL DEFAULT 0,
  schema_retries_used INTEGER NOT NULL DEFAULT 0,
  accepted_paper_id TEXT,
  envelope_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (run_id, stage, reviewer_seat, target_seat)
);
CREATE INDEX logical_calls_run_state ON logical_calls(run_id, state);

CREATE TABLE envelopes (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  logical_call_id TEXT NOT NULL UNIQUE REFERENCES logical_calls(id) ON DELETE CASCADE,
  stage TEXT NOT NULL,
  instructions TEXT NOT NULL,
  messages_json TEXT NOT NULL,
  schema_id TEXT NOT NULL,
  allowed_view_ids_json TEXT NOT NULL,
  alias_map_json TEXT NOT NULL,
  digest TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE attempts (
  id TEXT PRIMARY KEY,
  logical_call_id TEXT NOT NULL REFERENCES logical_calls(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  serialized_body_redacted TEXT NOT NULL,
  dispatched_at TEXT NOT NULL,
  first_byte_at TEXT,
  completed_at TEXT,
  provider_request_id TEXT,
  idempotency_key TEXT,
  outcome TEXT,
  finish_reason TEXT,
  usage_json TEXT,
  raw_output_bounded TEXT,
  error_category TEXT,
  error_sanitized TEXT,
  UNIQUE (logical_call_id, ordinal)
);
CREATE INDEX attempts_call ON attempts(logical_call_id);

CREATE TABLE papers (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  author_seat INTEGER NOT NULL,
  target_paper_id TEXT,
  parent_paper_id TEXT,
  schema_version TEXT NOT NULL,
  body_json TEXT NOT NULL,
  digest TEXT NOT NULL,
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  created_at TEXT NOT NULL
);
CREATE INDEX papers_run_kind ON papers(run_id, kind);

CREATE TABLE artifact_views (
  id TEXT PRIMARY KEY,
  paper_id TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  transform_version TEXT NOT NULL,
  view_text TEXT NOT NULL,
  digest TEXT NOT NULL,
  possibly_identifiable INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE run_events (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_id, seq)
);

CREATE TABLE budget_entries (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  logical_call_id TEXT REFERENCES logical_calls(id) ON DELETE CASCADE,
  attempt_id TEXT REFERENCES attempts(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  amount_usd_micro INTEGER NOT NULL,
  price_ref TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX budget_run ON budget_entries(run_id);

CREATE TABLE credentials_meta (
  ref TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  label TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_tested_at TEXT,
  last_test_result TEXT
);

-- Full-text search over questions for History (UI-07).
CREATE VIRTUAL TABLE runs_fts USING fts5(run_id UNINDEXED, question, final_answer);

-- Maintenance flag: the only way to update/delete immutable rows (DATA-01, DATA-03).
CREATE TABLE maintenance (id INTEGER PRIMARY KEY CHECK (id = 1), enabled INTEGER NOT NULL DEFAULT 0);
INSERT INTO maintenance (id, enabled) VALUES (1, 0);

CREATE TRIGGER papers_no_update BEFORE UPDATE ON papers
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'papers are immutable'); END;
CREATE TRIGGER papers_no_delete BEFORE DELETE ON papers
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'papers are immutable'); END;
CREATE TRIGGER envelopes_no_update BEFORE UPDATE ON envelopes
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'envelopes are immutable'); END;
CREATE TRIGGER envelopes_no_delete BEFORE DELETE ON envelopes
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'envelopes are immutable'); END;
CREATE TRIGGER views_no_update BEFORE UPDATE ON artifact_views
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'views are immutable'); END;
CREATE TRIGGER views_no_delete BEFORE DELETE ON artifact_views
  WHEN (SELECT enabled FROM maintenance WHERE id = 1) = 0
  BEGIN SELECT RAISE(ABORT, 'views are immutable'); END;
