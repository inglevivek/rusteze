# rusteze

## Batch Upload API

The application now supports batch file uploading for ingestion via the `/api/ingest/batch` endpoint.

### `POST /api/ingest/batch`

Accepts a `multipart/form-data` request with:
- `case_id` (text): The ID of the case to ingest documents for.
- `document` (file): Repeated fields for multiple files (e.g., PDF, JPG, PNG).

**Behavior:**
1. Text is extracted from all uploaded documents sequentially.
2. The extracted text from all files is aggregated into a single document string.
3. The aggregated text is embedded in Qdrant and saved to Postgres under the single `case_id`.
4. A single batch-level adjudication report is generated for the aggregated content.

**Response:**
Returns a JSON object detailing the status of each file in the batch.

```json
{
  "case_id": "CASE123",
  "results": [
    {
      "file_name": "scan1.pdf",
      "status": "completed",
      "report": { /* JSON Adjudication Report */ },
      "error": null
    },
    {
      "file_name": "scan2.png",
      "status": "failed",
      "report": null,
      "error": "OCR Pipeline Error: ..."
    }
  ]
}
```

## Frontend Workspace

The UI in `public/` provides a comprehensive 3-panel layout:
1. **Left Sidebar:** Case list and a drag-and-drop batch upload queue.
2. **Center Chat:** Interactive interrogation interface with Markdown rendering and code-copy functionality.
3. **Right Details:** Collapsible panel for viewing the generated JSON adjudication reports.