use rusqlite::{Connection, Result};

#[derive(Debug, Clone)]
pub struct ClinicalEntity {
    pub concept_id: String,
    pub term_type: String,
    pub name: String,
}

pub fn search_dictionary(term: &str) -> Result<Option<ClinicalEntity>> {
    // Hardcoded path for local testing
    let conn = Connection::open("data/nrces_dict.db")?;

    // We use COLLATE NOCASE for case-insensitive matching
    let mut stmt = conn.prepare(
        "SELECT concept_id, term_type, name FROM dictionary WHERE name LIKE ?1 COLLATE NOCASE LIMIT 1"
    )?;

    let mut rows = stmt.query([format!("%{}%", term)])?;

    if let Some(row) = rows.next()? {
        Ok(Some(ClinicalEntity {
            concept_id: row.get(0)?,
            term_type: row.get(1)?,
            name: row.get(2)?,
        }))
    } else {
        Ok(None)
    }
}
