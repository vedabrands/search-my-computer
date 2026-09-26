use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySuiteMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryEntry {
    pub id: String,
    pub query: String,
    pub query_type: String, // "exact-name" | "keyword" | "semantic" | "mixed"
    pub expected_files: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySuite {
    #[serde(default)]
    pub metadata: Option<QuerySuiteMetadata>,
    #[serde(rename = "query")]
    pub queries: Vec<QueryEntry>,
}

impl QuerySuite {
    /// Loads a query suite from a TOML string.
    pub fn from_toml_str(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Loads a query suite from a file path.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let suite = Self::from_toml_str(&content)?;
        Ok(suite)
    }

    /// Loads default embedded query suite or overrides with `queries.local.toml` if present.
    pub fn load_auto<P: AsRef<Path>>(base_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let base = base_dir.as_ref();
        let local_path = base.join("queries.local.toml");
        if local_path.exists() {
            tracing::info!(path = ?local_path, "loading local query suite");
            return Self::from_file(local_path);
        }

        let default_path = base.join("queries.toml");
        if default_path.exists() {
            return Self::from_file(default_path);
        }

        // Fallback to embedded default queries
        let embedded = include_str!("../queries.toml");
        let suite = Self::from_toml_str(embedded)?;
        Ok(suite)
    }

    /// Returns queries grouped by category / query_type.
    pub fn group_by_type(&self) -> HashMap<String, Vec<QueryEntry>> {
        let mut groups: HashMap<String, Vec<QueryEntry>> = HashMap::new();
        for q in &self.queries {
            groups
                .entry(q.query_type.clone())
                .or_default()
                .push(q.clone());
        }
        groups
    }
}
