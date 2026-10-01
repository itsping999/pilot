use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use log::{info, warn};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SkillError {
    ConnectionRefNotFound(String, String),
    UnsupportedScheme(String),
    InvalidSql(String),
    ExecutionFailed(String),
    ParameterError(String),
    SkillNotFound(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionRefNotFound(name, env_key) => {
                write!(
                    f,
                    "connection reference '{name}' not found in environment (expected KP_DB_{env_key})"
                )
            }
            Self::UnsupportedScheme(url) => {
                write!(f, "unsupported database scheme in connection URL: {url}")
            }
            Self::InvalidSql(msg) => write!(f, "invalid SQL statement: {msg}"),
            Self::ExecutionFailed(msg) => write!(f, "database execution error: {msg}"),
            Self::ParameterError(msg) => write!(f, "parameter error: {msg}"),
            Self::SkillNotFound(name) => write!(f, "skill '{name}' not found"),
        }
    }
}

impl std::error::Error for SkillError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SqlSkillParameter {
    pub name: String,
    #[serde(default = "default_param_type")]
    pub param_type: String, // "string", "integer", "number", "boolean"
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

fn default_param_type() -> String {
    "string".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SqlSkill {
    pub name: String,
    pub description: String,
    pub connection_ref: String,
    pub sql_template: String,
    #[serde(default)]
    pub parameters: Vec<SqlSkillParameter>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub max_rows: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
    pub total_rows: usize,
    pub execution_ms: u64,
}

pub struct DataSourceManager {
    mysql_pools: RwLock<HashMap<String, Arc<mysql::Pool>>>,
    sqlite_memory_conns: RwLock<HashMap<String, Arc<Mutex<rusqlite::Connection>>>>,
}

impl Default for DataSourceManager {
    fn default() -> Self {
        Self::new()
    }
}

type RawQueryRows = (
    Vec<String>,
    Vec<serde_json::Map<String, serde_json::Value>>,
    usize,
);

impl DataSourceManager {
    pub fn new() -> Self {
        Self {
            mysql_pools: RwLock::new(HashMap::new()),
            sqlite_memory_conns: RwLock::new(HashMap::new()),
        }
    }

    /// Resolve connection URL from environment variable: KP_DB_<REF>
    pub fn resolve_connection_url(connection_ref: &str) -> Result<String, SkillError> {
        let env_key_upper = format!("KP_DB_{}", connection_ref.to_ascii_uppercase());
        if let Ok(url) = std::env::var(&env_key_upper) {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        let env_key_raw = format!("KP_DB_{connection_ref}");
        if let Ok(url) = std::env::var(&env_key_raw) {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        Err(SkillError::ConnectionRefNotFound(
            connection_ref.to_string(),
            connection_ref.to_ascii_uppercase(),
        ))
    }

    /// Execute a parameterized SQL query on MySQL or SQLite
    pub fn execute_query(
        &self,
        connection_ref: &str,
        sql: &str,
        params: &HashMap<String, serde_json::Value>,
        max_rows: usize,
    ) -> Result<QueryResult, SkillError> {
        validate_read_only_sql(sql)?;
        let bounded_sql = enforce_limit(sql, max_rows);

        let url = Self::resolve_connection_url(connection_ref)?;
        let start = Instant::now();

        let (columns, rows, total_rows) =
            if url.starts_with("mysql://") || url.starts_with("mariadb://") {
                self.execute_mysql(&url, &bounded_sql, params)?
            } else if url.starts_with("sqlite://") || url.starts_with("sqlite:") {
                let path = url
                    .trim_start_matches("sqlite://")
                    .trim_start_matches("sqlite:");
                self.execute_sqlite(connection_ref, path, &bounded_sql, params)?
            } else {
                return Err(SkillError::UnsupportedScheme(url));
            };

        let execution_ms = start.elapsed().as_millis() as u64;
        Ok(QueryResult {
            columns,
            rows,
            total_rows,
            execution_ms,
        })
    }

    fn execute_mysql(
        &self,
        url: &str,
        sql: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<RawQueryRows, SkillError> {
        let pool = {
            let pools = self
                .mysql_pools
                .read()
                .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
            if let Some(pool) = pools.get(url) {
                pool.clone()
            } else {
                drop(pools);
                let mut pools = self
                    .mysql_pools
                    .write()
                    .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
                let pool = Arc::new(mysql::Pool::new(url).map_err(|e| {
                    SkillError::ExecutionFailed(format!("Failed to connect to MySQL: {e}"))
                })?);
                pools.insert(url.to_string(), pool.clone());
                pool
            }
        };

        use mysql::prelude::*;
        let mut conn = pool.get_conn().map_err(|e| {
            SkillError::ExecutionFailed(format!(
                "Failed to acquire MySQL connection from pool: {e}"
            ))
        })?;

        // Convert params to mysql::Params::Named
        let mut mysql_params = Vec::new();
        for (k, v) in params {
            let mysql_val = json_to_mysql_value(v);
            mysql_params.push((k.as_bytes(), mysql_val));
        }

        let query_result = conn
            .exec_iter(
                sql,
                mysql::Params::Named(
                    mysql_params
                        .into_iter()
                        .map(|(k, v)| (k.to_vec(), v))
                        .collect(),
                ),
            )
            .map_err(|e| SkillError::ExecutionFailed(format!("MySQL execution failed: {e}")))?;

        let columns: Vec<String> = query_result
            .columns()
            .as_ref()
            .iter()
            .map(|col| col.name_str().to_string())
            .collect();

        let mut rows = Vec::new();
        for row_result in query_result {
            let row = row_result.map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
            let mut map = serde_json::Map::new();
            for (idx, col_name) in columns.iter().enumerate() {
                let val: Option<mysql::Value> = row.get_opt(idx).and_then(Result::ok);
                let json_val = match val {
                    Some(v) => mysql_value_to_json(v),
                    None => serde_json::Value::Null,
                };
                map.insert(col_name.clone(), json_val);
            }
            rows.push(map);
        }

        let total_rows = rows.len();
        Ok((columns, rows, total_rows))
    }

    fn execute_sqlite(
        &self,
        connection_ref: &str,
        path: &str,
        sql: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<RawQueryRows, SkillError> {
        let (columns, rows, total) = if path == ":memory:" {
            let conns = self
                .sqlite_memory_conns
                .read()
                .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
            let conn_arc = if let Some(c) = conns.get(connection_ref) {
                c.clone()
            } else {
                drop(conns);
                let mut conns = self
                    .sqlite_memory_conns
                    .write()
                    .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
                let c = rusqlite::Connection::open_in_memory()
                    .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
                let arc = Arc::new(Mutex::new(c));
                conns.insert(connection_ref.to_string(), arc.clone());
                arc
            };
            let conn = conn_arc
                .lock()
                .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
            execute_sqlite_conn(&conn, sql, params)?
        } else {
            let conn = rusqlite::Connection::open(path).map_err(|e| {
                SkillError::ExecutionFailed(format!("Failed to open SQLite at {path}: {e}"))
            })?;
            execute_sqlite_conn(&conn, sql, params)?
        };

        Ok((columns, rows, total))
    }

    /// Helper for testing: inject an in-memory SQLite connection for a given connection_ref
    #[cfg(test)]
    pub fn register_memory_sqlite(&self, connection_ref: &str, conn: rusqlite::Connection) {
        if let Ok(mut conns) = self.sqlite_memory_conns.write() {
            conns.insert(connection_ref.to_string(), Arc::new(Mutex::new(conn)));
        }
    }
}

fn execute_sqlite_conn(
    conn: &rusqlite::Connection,
    sql: &str,
    params: &HashMap<String, serde_json::Value>,
) -> Result<RawQueryRows, SkillError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| SkillError::ExecutionFailed(format!("SQLite prepare failed: {e}")))?;

    let column_count = stmt.column_count();
    let columns: Vec<String> = (0..column_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    // Map named params (:name or name)
    let mut named_values: Vec<(String, rusqlite::types::Value)> = Vec::new();
    for (k, v) in params {
        let param_key = if k.starts_with(':') {
            k.clone()
        } else {
            format!(":{k}")
        };
        named_values.push((param_key, json_to_sqlite_value(v)));
    }

    let param_refs: Vec<(&str, &dyn rusqlite::types::ToSql)> = named_values
        .iter()
        .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::types::ToSql))
        .collect();

    let mut rows_iter = stmt
        .query(param_refs.as_slice())
        .map_err(|e| SkillError::ExecutionFailed(format!("SQLite query failed: {e}")))?;

    let mut rows = Vec::new();
    while let Some(row) = rows_iter
        .next()
        .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?
    {
        let mut map = serde_json::Map::new();
        for (idx, col_name) in columns.iter().enumerate() {
            let val: rusqlite::types::Value = row
                .get(idx)
                .map_err(|e| SkillError::ExecutionFailed(e.to_string()))?;
            map.insert(col_name.clone(), sqlite_value_to_json(val));
        }
        rows.push(map);
    }

    let total = rows.len();
    Ok((columns, rows, total))
}

/// Strictly validate that the SQL statement is read-only.
pub fn validate_read_only_sql(sql: &str) -> Result<(), SkillError> {
    let clean = strip_sql_comments(sql).trim().to_string();
    if clean.is_empty() {
        return Err(SkillError::InvalidSql("SQL query is empty".into()));
    }

    // Check multiple statements
    let semicolon_count = clean.chars().filter(|&c| c == ';').count();
    if semicolon_count > 1 || (semicolon_count == 1 && !clean.ends_with(';')) {
        return Err(SkillError::InvalidSql(
            "Multiple statements separated by semicolon are forbidden".into(),
        ));
    }

    let clean_no_semi = clean.trim_end_matches(';').trim();
    let upper = clean_no_semi.to_ascii_uppercase();

    // Must start with SELECT or WITH or EXPLAIN
    if !upper.starts_with("SELECT") && !upper.starts_with("WITH") && !upper.starts_with("EXPLAIN") {
        return Err(SkillError::InvalidSql(
            "Query must begin with SELECT, WITH, or EXPLAIN".into(),
        ));
    }

    // Forbid dangerous keywords as tokens
    const FORBIDDEN_WORDS: &[&str] = &[
        "INSERT",
        "UPDATE",
        "DELETE",
        "DROP",
        "ALTER",
        "TRUNCATE",
        "REPLACE",
        "CREATE",
        "GRANT",
        "REVOKE",
        "EXEC",
        "EXECUTE",
        "LOCK",
        "CALL",
        "INTO OUTFILE",
        "INTO DUMPFILE",
        "LOAD_FILE",
    ];

    for &word in FORBIDDEN_WORDS {
        if contains_word_or_phrase(&upper, word) {
            return Err(SkillError::InvalidSql(format!(
                "Forbidden modification keyword '{word}' detected in query"
            )));
        }
    }

    Ok(())
}

fn contains_word_or_phrase(sql_upper: &str, target: &str) -> bool {
    let target_upper = target.to_ascii_uppercase();
    if target_upper.contains(' ') {
        return sql_upper.contains(&target_upper);
    }

    // Check whole word boundary
    for word in sql_upper.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if word == target_upper {
            return true;
        }
    }
    false
}

fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' && chars.peek() == Some(&'-') {
            chars.next();
            // skip until newline
            for next_c in chars.by_ref() {
                if next_c == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            // skip until */
            while let Some(next_c) = chars.next() {
                if next_c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn enforce_limit(sql: &str, max_rows: usize) -> String {
    let clean = sql.trim().trim_end_matches(';').trim();
    let upper = clean.to_ascii_uppercase();
    if !upper.contains("LIMIT") {
        format!("{clean} LIMIT {max_rows}")
    } else {
        clean.to_string()
    }
}

fn json_to_mysql_value(val: &serde_json::Value) -> mysql::Value {
    match val {
        serde_json::Value::Null => mysql::Value::NULL,
        serde_json::Value::Bool(b) => mysql::Value::Int(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                mysql::Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                mysql::Value::UInt(u)
            } else if let Some(f) = n.as_f64() {
                mysql::Value::Double(f)
            } else {
                mysql::Value::NULL
            }
        }
        serde_json::Value::String(s) => mysql::Value::Bytes(s.as_bytes().to_vec()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            mysql::Value::Bytes(serde_json::to_string(val).unwrap_or_default().into_bytes())
        }
    }
}

fn mysql_value_to_json(val: mysql::Value) -> serde_json::Value {
    match val {
        mysql::Value::NULL => serde_json::Value::Null,
        mysql::Value::Bytes(b) => {
            if let Ok(s) = String::from_utf8(b.clone()) {
                serde_json::Value::String(s)
            } else {
                serde_json::Value::String(String::from_utf8_lossy(&b).to_string())
            }
        }
        mysql::Value::Int(i) => serde_json::json!(i),
        mysql::Value::UInt(u) => serde_json::json!(u),
        mysql::Value::Float(f) => serde_json::json!(f),
        mysql::Value::Double(d) => serde_json::json!(d),
        mysql::Value::Date(y, m, d, h, i, s, _u) => {
            serde_json::Value::String(format!("{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}"))
        }
        mysql::Value::Time(is_neg, d, h, m, s, _u) => {
            let sign = if is_neg { "-" } else { "" };
            serde_json::Value::String(format!("{sign}{d}d {h:02}:{m:02}:{s:02}"))
        }
    }
}

fn json_to_sqlite_value(val: &serde_json::Value) -> rusqlite::types::Value {
    match val {
        serde_json::Value::Null => rusqlite::types::Value::Null,
        serde_json::Value::Bool(b) => rusqlite::types::Value::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rusqlite::types::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                rusqlite::types::Value::Real(f)
            } else {
                rusqlite::types::Value::Null
            }
        }
        serde_json::Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            rusqlite::types::Value::Text(serde_json::to_string(val).unwrap_or_default())
        }
    }
}

fn sqlite_value_to_json(val: rusqlite::types::Value) -> serde_json::Value {
    match val {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::json!(i),
        rusqlite::types::Value::Real(f) => serde_json::json!(f),
        rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(String::from_utf8_lossy(&b).to_string())
        }
    }
}

/// Skill registry holding loaded SQL skills.
#[derive(Debug, Clone, Default)]
pub struct SqlSkillRegistry {
    skills: HashMap<String, SqlSkill>,
}

impl SqlSkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    pub fn register(&mut self, skill: SqlSkill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    pub fn get(&self, name: &str) -> Option<&SqlSkill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&SqlSkill> {
        let mut list: Vec<&SqlSkill> = self.skills.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let skills: Vec<SqlSkill> = serde_json::from_str(&content)?;
        let mut registry = Self::new();
        for skill in skills {
            registry.register(skill);
        }
        Ok(registry)
    }

    pub fn load_from_env_or_default(custom_path: Option<&Path>) -> Self {
        let candidate_paths = if let Some(p) = custom_path {
            vec![p.to_path_buf()]
        } else if let Ok(env_path) = std::env::var("KNOWLEDGE_PILOT_SKILLS_PATH") {
            vec![PathBuf::from(env_path)]
        } else {
            vec![
                PathBuf::from("./config/skills.json"),
                PathBuf::from("./skills.json"),
            ]
        };

        for path in candidate_paths {
            if path.exists() {
                match Self::load_from_file(&path) {
                    Ok(registry) => {
                        info!(
                            "Loaded {} SQL skill(s) from {}",
                            registry.skills.len(),
                            path.display()
                        );
                        return registry;
                    }
                    Err(err) => {
                        warn!("Failed to load SQL skills from {}: {err}", path.display());
                    }
                }
            }
        }

        info!("No skills file found; initializing empty SQL skill registry");
        Self::new()
    }
}

/// Format query results as a friendly Markdown table or summary for presentation
pub fn format_query_result_markdown(result: &QueryResult) -> String {
    if result.rows.is_empty() {
        return "没有查询到符合条件的数据。".to_string();
    }

    let mut out = String::new();
    out.push_str(&format!(
        "共查询到 {} 条数据（耗时 {}ms）：\n\n",
        result.total_rows, result.execution_ms
    ));

    // Markdown table header
    out.push_str("| ");
    for col in &result.columns {
        out.push_str(col);
        out.push_str(" | ");
    }
    out.push('\n');

    out.push_str("| ");
    for _ in &result.columns {
        out.push_str("--- | ");
    }
    out.push('\n');

    // Rows
    for row in &result.rows {
        out.push_str("| ");
        for col in &result.columns {
            let val = row.get(col).unwrap_or(&serde_json::Value::Null);
            let display_str = match val {
                serde_json::Value::Null => "-",
                serde_json::Value::String(s) => s.as_str(),
                _ => &val.to_string(),
            };
            out.push_str(display_str);
            out.push_str(" | ");
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_read_only_sql() {
        assert!(validate_read_only_sql("SELECT * FROM users").is_ok());
        assert!(
            validate_read_only_sql("SELECT id, name FROM users WHERE id = :id LIMIT 10").is_ok()
        );
        assert!(
            validate_read_only_sql("WITH cte AS (SELECT * FROM orders) SELECT * FROM cte").is_ok()
        );
        assert!(validate_read_only_sql("EXPLAIN SELECT 1").is_ok());
        assert!(validate_read_only_sql("-- query comment\nSELECT * FROM t").is_ok());

        // Dangerous queries must fail
        assert!(validate_read_only_sql("INSERT INTO users VALUES (1, 'bad')").is_err());
        assert!(validate_read_only_sql("UPDATE users SET name = 'bad'").is_err());
        assert!(validate_read_only_sql("DELETE FROM users").is_err());
        assert!(validate_read_only_sql("DROP TABLE users").is_err());
        assert!(validate_read_only_sql("ALTER TABLE users ADD col INT").is_err());
        assert!(validate_read_only_sql("TRUNCATE TABLE users").is_err());
        assert!(validate_read_only_sql("SELECT * FROM users; DROP TABLE users;").is_err());
        assert!(validate_read_only_sql("SELECT * FROM users WHERE id = 1; SELECT 2").is_err());
    }

    #[test]
    fn test_enforce_limit() {
        assert_eq!(
            enforce_limit("SELECT * FROM t", 50),
            "SELECT * FROM t LIMIT 50"
        );
        assert_eq!(
            enforce_limit("SELECT * FROM t LIMIT 10", 50),
            "SELECT * FROM t LIMIT 10"
        );
        assert_eq!(
            enforce_limit("SELECT * FROM t;", 50),
            "SELECT * FROM t LIMIT 50"
        );
    }

    #[test]
    fn test_sqlite_skill_execution() {
        let manager = DataSourceManager::new();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT, email TEXT, status TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users VALUES (1001, 'alice', 'alice@corp.internal', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users VALUES (1002, 'bob', 'bob@corp.internal', 'disabled')",
            [],
        )
        .unwrap();

        manager.register_memory_sqlite("TEST", conn);
        unsafe {
            std::env::set_var("KP_DB_TEST", "sqlite://:memory:");
        }

        let mut params = HashMap::new();
        params.insert("user_id".to_string(), serde_json::json!(1001));

        let res = manager
            .execute_query(
                "TEST",
                "SELECT id, username, email, status FROM users WHERE id = :user_id",
                &params,
                10,
            )
            .unwrap();

        assert_eq!(res.total_rows, 1);
        assert_eq!(res.columns, vec!["id", "username", "email", "status"]);
        assert_eq!(res.rows[0]["username"], "alice");
        assert_eq!(res.rows[0]["status"], "active");

        let markdown = format_query_result_markdown(&res);
        assert!(markdown.contains("alice"));
        assert!(markdown.contains("active"));
    }
}
