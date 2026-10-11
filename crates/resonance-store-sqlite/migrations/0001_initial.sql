-- Initial schema: nodes, edges, blobs, files, ops, meta (§8.2, mvp.md §4).
-- An internal detail of the SQLite adapter; never part of any interface (FR-161).
-- Anchors live in code-unit node properties; `nodes_path` indexes them by file.

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE nodes (
    graph   TEXT NOT NULL,
    id      TEXT NOT NULL,
    type    TEXT NOT NULL,
    layer   TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0,
    props   TEXT NOT NULL,
    PRIMARY KEY (graph, id)
);
CREATE INDEX nodes_type ON nodes (graph, type);
CREATE INDEX nodes_path ON nodes (graph, json_extract(props, '$.path'));

CREATE TABLE edges (
    graph   TEXT NOT NULL,
    id      TEXT NOT NULL,
    type    TEXT NOT NULL,
    src     TEXT NOT NULL,
    dst     TEXT NOT NULL,
    layer   TEXT NOT NULL,
    status  TEXT NOT NULL,
    removed INTEGER NOT NULL DEFAULT 0,
    props   TEXT NOT NULL,
    PRIMARY KEY (graph, id)
);
CREATE INDEX edges_src ON edges (graph, src, type);
CREATE INDEX edges_dst ON edges (graph, dst, type);
CREATE INDEX edges_status ON edges (graph, status);

CREATE TABLE blobs (
    hash  TEXT PRIMARY KEY,
    size  INTEGER NOT NULL,
    bytes BLOB
);

CREATE TABLE files (
    graph       TEXT NOT NULL,
    path        TEXT NOT NULL,
    blob        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    recorded_ns INTEGER NOT NULL,
    PRIMARY KEY (graph, path)
);

CREATE TABLE ops (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    graph      TEXT NOT NULL,
    op_id      TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL,
    actor      TEXT NOT NULL,
    hlc        TEXT NOT NULL,
    change_set TEXT NOT NULL,
    schema     INTEGER NOT NULL,
    payload    TEXT NOT NULL
);
CREATE INDEX ops_change_set ON ops (change_set);
