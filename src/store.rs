use std::sync::{Mutex, MutexGuard};

use rusqlite::types::Type;
use rusqlite::{Connection, ToSql, params, params_from_iter};

use crate::embedding::{decode_vector, encode_vector};
use crate::models::{
    AnswerConfidence, Chunk, Citation, ConfidenceLevel, Document, DocumentSummary,
    QueryHistoryEntry,
};

pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
        }
    }

    pub fn init(&self) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source TEXT NOT NULL,
                access_scope TEXT NOT NULL DEFAULT 'public',
                text TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                section TEXT NOT NULL DEFAULT '',
                text TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                chunk_id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                dimensions INTEGER NOT NULL,
                vector BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS rag_queries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                question TEXT NOT NULL,
                answer TEXT NOT NULL,
                citations_json TEXT NOT NULL,
                confidence_level TEXT NOT NULL DEFAULT 'medium',
                confidence_score REAL NOT NULL DEFAULT 0,
                confidence_reason TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )?;
        ensure_column(
            &connection,
            "documents",
            "access_scope",
            "TEXT NOT NULL DEFAULT 'public'",
        )?;
        ensure_column(&connection, "chunks", "title", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&connection, "chunks", "section", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(
            &connection,
            "rag_queries",
            "confidence_level",
            "TEXT NOT NULL DEFAULT 'medium'",
        )?;
        ensure_column(
            &connection,
            "rag_queries",
            "confidence_score",
            "REAL NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &connection,
            "rag_queries",
            "confidence_reason",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        Ok(())
    }

    pub fn replace_document_with_embeddings(
        &self,
        document: &Document,
        chunks: &[(Chunk, Vec<f32>)],
        model: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            r#"
            INSERT INTO documents (id, title, source, access_scope, text)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                source = excluded.source,
                access_scope = excluded.access_scope,
                text = excluded.text
            "#,
            params![
                document.id,
                document.title,
                document.source,
                document.access_scope,
                document.text
            ],
        )?;
        transaction.execute(
            "DELETE FROM chunks WHERE document_id = ?1",
            params![document.id],
        )?;

        for (chunk, vector) in chunks {
            transaction.execute(
                r#"
                INSERT INTO chunks (id, document_id, chunk_index, title, section, text, source)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    chunk.id,
                    chunk.document_id,
                    chunk.chunk_index as i64,
                    chunk.document_title,
                    chunk.section,
                    chunk.text,
                    chunk.source
                ],
            )?;
            transaction.execute(
                r#"
                INSERT INTO embeddings (chunk_id, model, dimensions, vector)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![chunk.id, model, vector.len() as i64, encode_vector(vector)],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn is_document_up_to_date(
        &self,
        id: &str,
        title: &str,
        source: &str,
        access_scope: &str,
        text: &str,
        model: &str,
    ) -> rusqlite::Result<Option<usize>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT d.title, d.source, d.access_scope, d.text, COUNT(c.id) AS chunk_count,
                   COUNT(CASE WHEN e.model = ?2 THEN 1 END) AS model_embedding_count
            FROM documents d
            LEFT JOIN chunks c ON c.document_id = d.id
            LEFT JOIN embeddings e ON e.chunk_id = c.id
            WHERE d.id = ?1
            GROUP BY d.id
            "#,
        )?;

        let mut rows = statement.query(params![id, model])?;
        if let Some(row) = rows.next()? {
            let existing_title: String = row.get(0)?;
            let existing_source: String = row.get(1)?;
            let existing_access_scope: String = row.get(2)?;
            let existing_text: String = row.get(3)?;
            let chunk_count: i64 = row.get(4)?;
            let model_embedding_count: i64 = row.get(5)?;

            if existing_title == title
                && existing_source == source
                && existing_access_scope == access_scope
                && existing_text == text
                && chunk_count > 0
                && chunk_count == model_embedding_count
            {
                return Ok(Some(chunk_count as usize));
            }
        }

        Ok(None)
    }

    pub fn list_documents(&self) -> rusqlite::Result<Vec<DocumentSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT d.id, d.title, d.source, d.access_scope, COUNT(c.id) AS chunks, d.created_at
            FROM documents d
            LEFT JOIN chunks c ON c.document_id = d.id
            GROUP BY d.id, d.title, d.source, d.access_scope, d.created_at
            ORDER BY d.created_at DESC, d.id DESC
            "#,
        )?;

        let rows = statement.query_map([], |row| {
            Ok(DocumentSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                source: row.get(2)?,
                access_scope: row.get(3)?,
                chunks: row.get::<_, i64>(4)? as usize,
                created_at: row.get(5)?,
            })
        })?;

        rows.collect()
    }

    pub fn get_document(&self, id: &str) -> rusqlite::Result<Option<Document>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, title, source, access_scope, text FROM documents WHERE id = ?1 LIMIT 1",
        )?;
        let mut rows = statement.query(params![id])?;

        match rows.next()? {
            Some(row) => Ok(Some(Document {
                id: row.get(0)?,
                title: row.get(1)?,
                source: row.get(2)?,
                access_scope: row.get(3)?,
                text: row.get(4)?,
            })),
            None => Ok(None),
        }
    }

    pub fn list_document_chunks(&self, document_id: &str) -> rusqlite::Result<Vec<Chunk>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT c.id, c.document_id, c.chunk_index, c.title, c.section, c.text, c.source, d.access_scope
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.document_id = ?1
            ORDER BY c.chunk_index ASC
            "#,
        )?;

        let rows = statement.query_map(params![document_id], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                document_id: row.get(1)?,
                chunk_index: row.get::<_, i64>(2)? as usize,
                document_title: row.get(3)?,
                section: row.get(4)?,
                text: row.get(5)?,
                source: row.get(6)?,
                access_scope: row.get(7)?,
            })
        })?;

        rows.collect()
    }

    pub fn delete_document(&self, id: &str) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let deleted = connection.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    pub fn load_chunks_with_embeddings(
        &self,
        model: &str,
        dimensions: usize,
        allowed_scopes: &[String],
    ) -> rusqlite::Result<Vec<(Chunk, Vec<f32>)>> {
        if allowed_scopes.is_empty() {
            return Ok(Vec::new());
        }

        let connection = self.connection()?;
        let scope_placeholders = std::iter::repeat_n("?", allowed_scopes.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT c.id, c.document_id, c.chunk_index, c.title, c.section, c.text, c.source, d.access_scope, e.vector
            FROM chunks c
            JOIN embeddings e ON e.chunk_id = c.id
            JOIN documents d ON d.id = c.document_id
            WHERE e.model = ? AND e.dimensions = ? AND d.access_scope IN ({scope_placeholders})
            "#
        );
        let mut statement = connection.prepare(&sql)?;
        let dimensions = dimensions as i64;
        let mut query_params: Vec<&dyn ToSql> = vec![&model, &dimensions];
        for scope in allowed_scopes {
            query_params.push(scope);
        }

        let rows = statement.query_map(params_from_iter(query_params), |row| {
            let bytes: Vec<u8> = row.get(8)?;
            Ok((
                Chunk {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    chunk_index: row.get::<_, i64>(2)? as usize,
                    document_title: row.get(3)?,
                    section: row.get(4)?,
                    text: row.get(5)?,
                    source: row.get(6)?,
                    access_scope: row.get(7)?,
                },
                decode_vector(&bytes),
            ))
        })?;

        rows.collect()
    }

    pub fn record_answer(
        &self,
        question: &str,
        answer: &str,
        citations_json: &str,
        confidence_level: &str,
        confidence_score: f32,
        confidence_reason: &str,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO rag_queries (
                question,
                answer,
                citations_json,
                confidence_level,
                confidence_score,
                confidence_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                question,
                answer,
                citations_json,
                confidence_level,
                confidence_score,
                confidence_reason
            ],
        )?;
        Ok(())
    }

    pub fn list_query_history(&self, limit: usize) -> rusqlite::Result<Vec<QueryHistoryEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT id, question, answer, citations_json, confidence_level, confidence_score, confidence_reason, created_at
            FROM rag_queries
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;

        let rows = statement.query_map(params![limit as i64], |row| {
            let citations_json: String = row.get(3)?;
            let citations: Vec<Citation> =
                serde_json::from_str(&citations_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(err))
                })?;
            Ok(QueryHistoryEntry {
                id: row.get(0)?,
                question: row.get(1)?,
                answer: row.get(2)?,
                citations,
                confidence: AnswerConfidence {
                    level: ConfidenceLevel::from_str(row.get::<_, String>(4)?.as_str()),
                    score: row.get(5)?,
                    reason: row.get(6)?,
                },
                created_at: row.get(7)?,
            })
        })?;

        rows.collect()
    }

    fn connection(&self) -> rusqlite::Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| {
            rusqlite::Error::InvalidParameterName("sqlite connection lock poisoned".to_string())
        })
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    if table_has_column(connection, table, column)? {
        return Ok(());
    }

    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::SqliteStore;
    use crate::models::{
        Chunk, Citation, ConfidenceLevel, Document, allowed_access_scopes, default_access_scope,
    };

    fn store() -> SqliteStore {
        let connection = Connection::open_in_memory().expect("open sqlite");
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("enable foreign keys");
        let store = SqliteStore::new(connection);
        store.init().expect("init schema");
        store
    }

    #[test]
    fn document_lifecycle_keeps_chunks_queryable_and_deletable() {
        let store = store();
        let document = Document {
            id: "doc-1".to_string(),
            title: "Runbook".to_string(),
            source: "runbook.md".to_string(),
            access_scope: default_access_scope(),
            text: "KnowledgePilot stores local documents.".to_string(),
        };
        let chunk = Chunk {
            id: "doc-1:0".to_string(),
            document_id: "doc-1".to_string(),
            chunk_index: 0,
            document_title: document.title.clone(),
            section: "Overview".to_string(),
            text: document.text.clone(),
            source: document.source.clone(),
            access_scope: document.access_scope.clone(),
        };

        store
            .replace_document_with_embeddings(&document, &[(chunk, vec![1.0, 0.0])], "test")
            .expect("replace document");

        let documents = store.list_documents().expect("list documents");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].chunks, 1);

        let loaded = store
            .get_document("doc-1")
            .expect("get document")
            .expect("document exists");
        assert_eq!(loaded.title, "Runbook");

        let chunks = store
            .list_document_chunks("doc-1")
            .expect("list document chunks");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, "doc-1:0");
        assert_eq!(chunks[0].document_title, "Runbook");
        assert_eq!(chunks[0].section, "Overview");

        assert!(store.delete_document("doc-1").expect("delete document"));
        assert!(
            store
                .load_chunks_with_embeddings("test", 2, &allowed_access_scopes(&[]))
                .expect("load embeddings")
                .is_empty()
        );
        assert!(!store.delete_document("doc-1").expect("delete missing"));
    }

    #[test]
    fn load_chunks_filters_by_embedding_model_and_dimension() {
        let store = store();
        let document = Document {
            id: "doc-1".to_string(),
            title: "Runbook".to_string(),
            source: "runbook.md".to_string(),
            access_scope: default_access_scope(),
            text: "KnowledgePilot stores local documents.".to_string(),
        };
        let chunk = Chunk {
            id: "doc-1:0".to_string(),
            document_id: "doc-1".to_string(),
            chunk_index: 0,
            document_title: document.title.clone(),
            section: "Overview".to_string(),
            text: document.text.clone(),
            source: document.source.clone(),
            access_scope: document.access_scope.clone(),
        };

        store
            .replace_document_with_embeddings(&document, &[(chunk, vec![1.0, 0.0])], "model-a")
            .expect("replace document");

        assert_eq!(
            store
                .load_chunks_with_embeddings("model-a", 2, &allowed_access_scopes(&[]))
                .expect("load matching")
                .len(),
            1
        );
        assert!(
            store
                .load_chunks_with_embeddings("model-b", 2, &allowed_access_scopes(&[]))
                .expect("load wrong model")
                .is_empty()
        );
        assert!(
            store
                .load_chunks_with_embeddings("model-a", 3, &allowed_access_scopes(&[]))
                .expect("load wrong dimension")
                .is_empty()
        );
    }

    #[test]
    fn load_chunks_filters_by_document_access_scope() {
        let store = store();
        let public_document = Document {
            id: "doc-public".to_string(),
            title: "Public Runbook".to_string(),
            source: "public.md".to_string(),
            access_scope: default_access_scope(),
            text: "Public guidance".to_string(),
        };
        let finance_document = Document {
            id: "doc-finance".to_string(),
            title: "Finance Runbook".to_string(),
            source: "finance.md".to_string(),
            access_scope: "finance".to_string(),
            text: "Finance-only guidance".to_string(),
        };
        let public_chunk = Chunk {
            id: "doc-public:0".to_string(),
            document_id: public_document.id.clone(),
            chunk_index: 0,
            document_title: public_document.title.clone(),
            section: "Overview".to_string(),
            text: public_document.text.clone(),
            source: public_document.source.clone(),
            access_scope: public_document.access_scope.clone(),
        };
        let finance_chunk = Chunk {
            id: "doc-finance:0".to_string(),
            document_id: finance_document.id.clone(),
            chunk_index: 0,
            document_title: finance_document.title.clone(),
            section: "Overview".to_string(),
            text: finance_document.text.clone(),
            source: finance_document.source.clone(),
            access_scope: finance_document.access_scope.clone(),
        };

        store
            .replace_document_with_embeddings(
                &public_document,
                &[(public_chunk, vec![1.0, 0.0])],
                "model",
            )
            .expect("save public document");
        store
            .replace_document_with_embeddings(
                &finance_document,
                &[(finance_chunk, vec![0.0, 1.0])],
                "model",
            )
            .expect("save finance document");

        let public_only = store
            .load_chunks_with_embeddings("model", 2, &allowed_access_scopes(&[]))
            .expect("load public scope");
        assert_eq!(public_only.len(), 1);
        assert_eq!(public_only[0].0.document_id, "doc-public");

        let allowed = allowed_access_scopes(&["finance".to_string()]);
        let scoped = store
            .load_chunks_with_embeddings("model", 2, &allowed)
            .expect("load scoped");
        assert_eq!(scoped.len(), 2);
        assert!(
            scoped
                .iter()
                .any(|(chunk, _)| chunk.document_id == "doc-finance")
        );

        let empty: Vec<String> = Vec::new();
        assert!(
            store
                .load_chunks_with_embeddings("model", 2, &empty)
                .expect("load empty scopes")
                .is_empty()
        );
    }

    #[test]
    fn replace_document_with_embeddings_removes_stale_chunks() {
        let store = store();
        let document = Document {
            id: "doc-1".to_string(),
            title: "Runbook".to_string(),
            source: "runbook.md".to_string(),
            access_scope: default_access_scope(),
            text: "updated".to_string(),
        };
        let old_chunk = Chunk {
            id: "doc-1:0".to_string(),
            document_id: "doc-1".to_string(),
            chunk_index: 0,
            document_title: document.title.clone(),
            section: "Old".to_string(),
            text: "old".to_string(),
            source: document.source.clone(),
            access_scope: document.access_scope.clone(),
        };
        let new_chunk = Chunk {
            id: "doc-1:0".to_string(),
            document_id: "doc-1".to_string(),
            chunk_index: 0,
            document_title: document.title.clone(),
            section: "New".to_string(),
            text: "updated".to_string(),
            source: document.source.clone(),
            access_scope: document.access_scope.clone(),
        };

        store
            .replace_document_with_embeddings(
                &document,
                &[(old_chunk, vec![1.0, 0.0])],
                "old-model",
            )
            .expect("save old chunk");
        store
            .replace_document_with_embeddings(
                &document,
                &[(new_chunk, vec![0.0, 1.0])],
                "new-model",
            )
            .expect("replace document");

        assert!(
            store
                .load_chunks_with_embeddings("old-model", 2, &allowed_access_scopes(&[]))
                .expect("load old model")
                .is_empty()
        );
        let chunks = store
            .load_chunks_with_embeddings("new-model", 2, &allowed_access_scopes(&[]))
            .expect("load new model");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0.text, "updated");
        assert_eq!(chunks[0].0.section, "New");
    }

    #[test]
    fn init_migrates_old_chunk_metadata_columns() {
        let connection = Connection::open_in_memory().expect("open sqlite");
        connection
            .execute_batch(
                r#"
                CREATE TABLE chunks (
                    id TEXT PRIMARY KEY,
                    document_id TEXT NOT NULL,
                    chunk_index INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                "#,
            )
            .expect("create old chunks table");
        let store = SqliteStore::new(connection);
        store.init().expect("init schema");

        let document = Document {
            id: "doc-1".to_string(),
            title: "Runbook".to_string(),
            source: "runbook.md".to_string(),
            access_scope: default_access_scope(),
            text: "updated".to_string(),
        };
        let chunk = Chunk {
            id: "doc-1:0".to_string(),
            document_id: "doc-1".to_string(),
            chunk_index: 0,
            document_title: "Runbook".to_string(),
            section: "Migrated".to_string(),
            text: "updated".to_string(),
            source: document.source.clone(),
            access_scope: document.access_scope.clone(),
        };

        store
            .replace_document_with_embeddings(&document, &[(chunk, vec![0.0, 1.0])], "model")
            .expect("replace document");

        let chunks = store.list_document_chunks("doc-1").expect("list chunks");
        assert_eq!(chunks[0].document_title, "Runbook");
        assert_eq!(chunks[0].section, "Migrated");
    }

    #[test]
    fn list_query_history_returns_latest_entries_with_citations() {
        let store = store();
        let first_citation = Citation {
            document_id: "doc-1".to_string(),
            chunk_id: "doc-1:0".to_string(),
            document_title: "第一份文档".to_string(),
            section: "概述".to_string(),
            source: "first.md".to_string(),
            access_scope: default_access_scope(),
            score: 0.8,
        };
        let second_citation = Citation {
            document_id: "doc-2".to_string(),
            chunk_id: "doc-2:0".to_string(),
            document_title: "第二份文档".to_string(),
            section: "审计".to_string(),
            source: "second.md".to_string(),
            access_scope: default_access_scope(),
            score: 0.9,
        };

        store
            .record_answer(
                "第一个问题",
                "第一个答案",
                &serde_json::to_string(&vec![first_citation]).expect("serialize first citation"),
                "medium",
                0.5,
                "moderate_retrieval_support",
            )
            .expect("record first answer");
        store
            .record_answer(
                "第二个问题",
                "第二个答案",
                &serde_json::to_string(&vec![second_citation]).expect("serialize second citation"),
                "high",
                0.9,
                "strong_retrieval_support",
            )
            .expect("record second answer");

        let history = store.list_query_history(1).expect("list query history");

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].question, "第二个问题");
        assert_eq!(history[0].answer, "第二个答案");
        assert_eq!(history[0].citations.len(), 1);
        assert_eq!(history[0].citations[0].source, "second.md");
        assert_eq!(history[0].confidence.level, ConfidenceLevel::High);
        assert_eq!(history[0].confidence.reason, "strong_retrieval_support");
    }

    #[test]
    fn is_document_up_to_date_detects_changes_and_model_match() {
        let store = store();
        let document = Document {
            id: "doc-hash-test".to_string(),
            title: "Hash Test".to_string(),
            source: "test.md".to_string(),
            access_scope: "public".to_string(),
            text: "Hello world text".to_string(),
        };
        let chunk = Chunk {
            id: "doc-hash-test:0".to_string(),
            document_id: "doc-hash-test".to_string(),
            chunk_index: 0,
            document_title: "Hash Test".to_string(),
            section: "".to_string(),
            text: "Hello world text".to_string(),
            source: "test.md".to_string(),
            access_scope: "public".to_string(),
        };
        let vector = vec![0.1f32; 4];

        // Initially document is not in store
        let up_to_date = store
            .is_document_up_to_date(
                &document.id,
                &document.title,
                &document.source,
                &document.access_scope,
                &document.text,
                "test-model",
            )
            .expect("check up to date");
        assert_eq!(up_to_date, None);

        // Save document with embeddings
        store
            .replace_document_with_embeddings(&document, &[(chunk, vector)], "test-model")
            .expect("save document");

        // Now it should report up to date with 1 chunk
        let up_to_date = store
            .is_document_up_to_date(
                &document.id,
                &document.title,
                &document.source,
                &document.access_scope,
                &document.text,
                "test-model",
            )
            .expect("check up to date");
        assert_eq!(up_to_date, Some(1));

        // If text changes, it reports None
        let up_to_date = store
            .is_document_up_to_date(
                &document.id,
                &document.title,
                &document.source,
                &document.access_scope,
                "Changed text",
                "test-model",
            )
            .expect("check up to date");
        assert_eq!(up_to_date, None);

        // If model changes, it reports None
        let up_to_date = store
            .is_document_up_to_date(
                &document.id,
                &document.title,
                &document.source,
                &document.access_scope,
                &document.text,
                "different-model",
            )
            .expect("check up to date");
        assert_eq!(up_to_date, None);
    }
}
