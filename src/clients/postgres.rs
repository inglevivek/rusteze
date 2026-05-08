use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use std::error::Error;
use std::str::FromStr;
use tokio_postgres::NoTls;

#[derive(Debug, Clone)]
pub struct ClinicalEntity {
    pub concept_id: String,
    pub term_type: String,
    pub name: String,
    pub generic_concept_id: Option<String>,
}

pub async fn establish_pool(pg_url: &str) -> Pool {
    let mut cfg = Config::new();
    let parsed_config = tokio_postgres::Config::from_str(pg_url).unwrap();

    cfg.host = match &parsed_config.get_hosts()[0] {
        tokio_postgres::config::Host::Tcp(h) => Some(h.clone()),
        #[cfg(unix)]
        tokio_postgres::config::Host::Unix(p) => Some(p.to_string_lossy().into_owned()),
    };
    cfg.user = parsed_config.get_user().map(|s| s.to_string());
    cfg.password = parsed_config
        .get_password()
        .map(|b| std::str::from_utf8(b).unwrap_or("").to_string());
    cfg.dbname = parsed_config.get_dbname().map(|s| s.to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });

    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("Failed to create Postgres connection pool")
}

pub async fn search_dictionary(
    pool: &Pool,
    term: &str,
) -> Result<Option<ClinicalEntity>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;

    // ILIKE performs a case-insensitive match in Postgres
    let stmt = client
        .prepare("SELECT concept_id, term_type, name, generic_concept_id FROM dictionary WHERE name ILIKE $1 LIMIT 1")
        .await?;
    let query_term = format!("%{}%", term);

    let rows = client.query(&stmt, &[&query_term]).await?;

    if let Some(row) = rows.first() {
        Ok(Some(ClinicalEntity {
            concept_id: row.get(0),
            term_type: row.get(1),
            name: row.get(2),
            generic_concept_id: row.get(3),
        }))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaseSummary {
    pub case_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Case {
    pub case_id: String,
    pub document_text: String,
    pub adjudication_report: Option<serde_json::Value>,
    pub created_at: String,
}

pub async fn save_case(
    pool: &Pool,
    case_id: &str,
    document_text: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let stmt = client
        .prepare("INSERT INTO cases (case_id, document_text) VALUES ($1, $2) ON CONFLICT (case_id) DO UPDATE SET document_text = EXCLUDED.document_text")
        .await?;
    client.execute(&stmt, &[&case_id, &document_text]).await?;
    Ok(())
}

pub async fn update_case_report(
    pool: &Pool,
    case_id: &str,
    report: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let stmt = client
        .prepare("UPDATE cases SET adjudication_report = $1::TEXT::JSONB WHERE case_id = $2")
        .await?;
    client.execute(&stmt, &[&report, &case_id]).await?;
    Ok(())
}

pub async fn get_case(
    pool: &Pool,
    case_id: &str,
) -> Result<Option<Case>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let stmt = client
        .prepare("SELECT case_id, document_text, CAST(adjudication_report AS TEXT), CAST(created_at AS TEXT) FROM cases WHERE case_id = $1")
        .await?;
    let rows = client.query(&stmt, &[&case_id]).await?;

    if let Some(row) = rows.first() {
        let report_str: Option<String> = row.get(2);
        let report = if let Some(r_str) = report_str {
            serde_json::from_str(&r_str).unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };

        Ok(Some(Case {
            case_id: row.get(0),
            document_text: row.get(1),
            adjudication_report: if report.is_null() { None } else { Some(report) },
            created_at: row.get(3),
        }))
    } else {
        Ok(None)
    }
}

pub async fn list_cases(
    pool: &Pool,
) -> Result<Vec<CaseSummary>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let stmt = client
        .prepare("SELECT case_id, CAST(created_at AS TEXT) FROM cases ORDER BY created_at DESC LIMIT 50")
        .await?;
    let rows = client.query(&stmt, &[]).await?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(CaseSummary {
            case_id: row.get(0),
            created_at: row.get(1),
        });
    }

    Ok(summaries)
}
