CREATE TABLE history_lineage (
  entity_id TEXT PRIMARY KEY REFERENCES entities(id),
  payload TEXT NOT NULL
);
CREATE TABLE causal_links (
  record_id TEXT PRIMARY KEY,
  payload TEXT NOT NULL
);
CREATE TABLE history_baselines (
  record_id TEXT PRIMARY KEY REFERENCES causal_links(record_id),
  payload TEXT NOT NULL
);
CREATE TABLE history_merges (
  record_id TEXT PRIMARY KEY REFERENCES causal_links(record_id),
  payload TEXT NOT NULL
);
