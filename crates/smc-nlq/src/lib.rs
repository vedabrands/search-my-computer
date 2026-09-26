//! Natural language query parsing.
//!
//! Extracts date filters, file type constraints, location hints, and intent signals from queries
//! like "find that PDF about AI agents I downloaded last month".

pub mod parser;

pub use parser::{ParsedQuery, parse_query, parse_query_with_ref_time};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_query() {
        let q = ParsedQuery::plain("hello world");
        assert_eq!(q.text, "hello world");
        assert!(q.file_types.is_empty());
    }
}
