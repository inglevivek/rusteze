use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use std::error::Error;
use std::str::FromStr;
use tokio_postgres::NoTls;

#[derive(Debug, Clone)]
pub struct ClinicalEntity {
    pub concept_id: String,   // Will hold the snomed_id after linker resolution
    pub snomed_id: Option<String>, // Raw snomed_id from Postgres dictionary
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

    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("Failed to create Postgres connection pool");
    tracing::info!(
        "╔══ [Postgres] Pool established ══════════════════════════════╗\n  host={} db={} user={}\n╚═════════════════════════════════════════════════════════════╝",
        cfg.host.as_deref().unwrap_or("?"),
        cfg.dbname.as_deref().unwrap_or("?"),
        cfg.user.as_deref().unwrap_or("?")
    );
    pool
}

pub async fn search_dictionary(
    pool: &Pool,
    term: &str,
) -> Result<Option<ClinicalEntity>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;

    let sql = "SELECT concept_id, term_type, name, generic_concept_id, snomed_id FROM dictionary WHERE name ILIKE $1 LIMIT 1";
    let query_term = format!("%{}%", term);

    tracing::info!(
        "┌── [Postgres ▶ SEND] search_dictionary ─────────────────────\n│  SQL : {}\n│  term: {:?}\n└────────────────────────────────────────────────────────────",
        sql,
        query_term
    );

    let stmt = client.prepare(sql).await?;
    let rows = client.query(&stmt, &[&query_term]).await?;

    if let Some(row) = rows.first() {
        let entity = ClinicalEntity {
            concept_id: row.get(0),
            snomed_id: row.try_get(4).ok(),
            term_type: row.get(1),
            name: row.get(2),
            generic_concept_id: row.get(3),
        };
        tracing::info!(
            "└── [Postgres ◀ RECV] search_dictionary ─────────────────────\n│  ✅ HIT  name='{}' concept_id='{}' snomed_id={:?} type='{}'\n└────────────────────────────────────────────────────────────",
            entity.name,
            entity.concept_id,
            entity.snomed_id,
            entity.term_type
        );
        Ok(Some(entity))
    } else {
        tracing::warn!(
            "└── [Postgres ◀ RECV] search_dictionary ─────────────────────\n│  ❌ MISS  no match for {:?}\n└────────────────────────────────────────────────────────────",
            query_term
        );
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
    let sql = "INSERT INTO cases (case_id, document_text) VALUES ($1, $2) ON CONFLICT (case_id) DO UPDATE SET document_text = EXCLUDED.document_text";

    tracing::info!(
        "┌── [Postgres ▶ SEND] save_case ─────────────────────────────\n│  SQL    : INSERT/UPSERT into cases\n│  case_id: {}\n│  text_len: {} chars\n└────────────────────────────────────────────────────────────",
        case_id,
        document_text.len()
    );

    let stmt = client.prepare(sql).await?;
    let rows_affected = client.execute(&stmt, &[&case_id, &document_text]).await?;

    tracing::info!(
        "└── [Postgres ◀ RECV] save_case ─────────────────────────────\n│  ✅ rows_affected={}\n└────────────────────────────────────────────────────────────",
        rows_affected
    );
    Ok(())
}

pub async fn update_case_report(
    pool: &Pool,
    case_id: &str,
    report: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let sql = "UPDATE cases SET adjudication_report = $1::TEXT::JSONB WHERE case_id = $2";

    tracing::info!(
        "┌── [Postgres ▶ SEND] update_case_report ────────────────────\n│  SQL    : UPDATE cases (adjudication_report)\n│  case_id: {}\n│  report_len: {} chars\n└────────────────────────────────────────────────────────────",
        case_id,
        report.len()
    );

    let stmt = client.prepare(sql).await?;
    let rows_affected = client.execute(&stmt, &[&report, &case_id]).await?;

    tracing::info!(
        "└── [Postgres ◀ RECV] update_case_report ────────────────────\n│  ✅ rows_affected={}\n└────────────────────────────────────────────────────────────",
        rows_affected
    );
    Ok(())
}

pub async fn get_case(
    pool: &Pool,
    case_id: &str,
) -> Result<Option<Case>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let sql = "SELECT case_id, document_text, CAST(adjudication_report AS TEXT), CAST(created_at AS TEXT) FROM cases WHERE case_id = $1";

    tracing::info!(
        "┌── [Postgres ▶ SEND] get_case ──────────────────────────────\n│  SQL    : SELECT from cases\n│  case_id: {}\n└────────────────────────────────────────────────────────────",
        case_id
    );

    let stmt = client.prepare(sql).await?;
    let rows = client.query(&stmt, &[&case_id]).await?;

    if let Some(row) = rows.first() {
        let report_str: Option<String> = row.get(2);
        let report = if let Some(r_str) = report_str {
            serde_json::from_str(&r_str).unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };

        tracing::info!(
            "└── [Postgres ◀ RECV] get_case ──────────────────────────────\n│  ✅ found case_id='{}' has_report={}\n└────────────────────────────────────────────────────────────",
            case_id,
            !report.is_null()
        );

        Ok(Some(Case {
            case_id: row.get(0),
            document_text: row.get(1),
            adjudication_report: if report.is_null() { None } else { Some(report) },
            created_at: row.get(3),
        }))
    } else {
        tracing::warn!(
            "└── [Postgres ◀ RECV] get_case ──────────────────────────────\n│  ❌ not found: case_id='{}'\n└────────────────────────────────────────────────────────────",
            case_id
        );
        Ok(None)
    }
}

pub async fn list_cases(
    pool: &Pool,
) -> Result<Vec<CaseSummary>, Box<dyn Error + Send + Sync>> {
    let client = pool.get().await?;
    let sql = "SELECT case_id, CAST(created_at AS TEXT) FROM cases ORDER BY created_at DESC LIMIT 50";

    tracing::info!(
        "┌── [Postgres ▶ SEND] list_cases ────────────────────────────\n│  SQL: SELECT recent 50 cases\n└────────────────────────────────────────────────────────────"
    );

    let stmt = client.prepare(sql).await?;
    let rows = client.query(&stmt, &[]).await?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(CaseSummary {
            case_id: row.get(0),
            created_at: row.get(1),
        });
    }

    tracing::info!(
        "└── [Postgres ◀ RECV] list_cases ────────────────────────────\n│  ✅ returned {} cases\n└────────────────────────────────────────────────────────────",
        summaries.len()
    );

    Ok(summaries)
}
