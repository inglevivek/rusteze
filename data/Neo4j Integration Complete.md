# Neo4j Integration — Complete Reference
> **Scanned**: 2026-05-06 | **Project**: PCI System (Persistent Cognitive Inhabitant)

Neo4j serves as the **Symbolic Knowledge Graph** in PCI's Quad-DB memory stack, storing structured entities, concepts, events, contradictions, knowledge gaps (Lacunas), and subconscious dream synapses.

---

## 1. Infrastructure

### Docker Compose (`docker-compose.yml`)
```yaml
neo4j:
  image: neo4j:5
  container_name: pci_neo4j
  environment:
    NEO4J_AUTH: neo4j/pci_secret
  ports:
    - "7474:7474"   # Browser UI
    - "7687:7687"   # Bolt protocol
  volumes:
    - pci_neo4j_data:/data
  healthcheck:
    test: ["CMD-SHELL", "wget --no-verbose --tries=1 --spider http://localhost:7474 || exit 1"]
    interval: 15s
    timeout: 5s
    retries: 5
```

### Connection Credentials
| Context | URI | User | Password |
|---|---|---|---|
| **Backend (Python)** | `bolt://localhost:7687` | `neo4j` | `pci_secret` |
| **Frontend (Next.js)** | `bolt://localhost:7687` | `neo4j` | `pci_secret` |

### Dependencies
| Stack | Package | Version |
|---|---|---|
| Python (`requirements.txt`) | `neo4j` | `5.19.0` |
| Frontend (`package.json`) | `neo4j-driver` | `^6.0.1` |

### Environment Variables

**Backend** (`.env`) — No Neo4j-specific vars; hardcoded in code.

**Frontend** (`frontend/.env.local`):
```
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=pci_secret
```

---

## 2. Graph Schema

### Node Types
| Label | Properties | Created By |
|---|---|---|
| `User` | `id`, `psychological_profile` | `SyntheticMind`, `theory_of_mind_audit` |
| `Memory` | `id` | `SyntheticMind.sync_to_graph()` |
| `Concept` | `name`, `valence`, `electronegativity`, `phase_state`, `health` | `SyntheticMind.sync_to_graph()` (VBSE physics) |
| `Entity` | `name` | `SyntheticMind.sync_to_graph()` |
| `Event` | `name` | `SyntheticMind.sync_to_graph()` |
| `LacunaNode` | `id`, `topic`, `confidence`, `timestamp`, `resolved`, `resolved_at` | `MUR service` |

### Relationship Types
| Type | Direction | Properties | Created By |
|---|---|---|---|
| `[:INTERESTED_IN]` | `User → Concept` | — | `SyntheticMind.sync_to_graph()` |
| `[:ABOUT]` | `Memory → Concept` | — | `SyntheticMind.sync_to_graph()` |
| `[:INTERACTS_WITH]` | `User → Entity` | — | `SyntheticMind.sync_to_graph()` |
| `[:INVOLVES]` | `Event → Entity` | — | `SyntheticMind.sync_to_graph()` |
| `[:CONTRADICTS]` | `Memory → Memory` | `timestamp` | `CG service` |
| `[:SYNAPSE]` | `Concept → Concept` | `weight`, `activation_energy`, `timestamp` | `Dream Weaver` |
| `[:SYNAPSE]` | `User → Concept` | `insight`, `timestamp` | `Dream Weaver` (LLM fallback) |
| `[:HAS_KNOWLEDGE_GAP]` | `User → LacunaNode` | — | `MUR service` |

### Visual Schema
```
(:User) -[:INTERESTED_IN]→ (:Concept)
(:User) -[:INTERACTS_WITH]→ (:Entity)
(:Memory) -[:ABOUT]→ (:Concept)
(:Event) -[:INVOLVES]→ (:Entity)
(:Memory) -[:CONTRADICTS]→ (:Memory)
(:User) -[:HAS_KNOWLEDGE_GAP]→ (:LacunaNode)
(:Concept) -[:SYNAPSE {weight, activation_energy}]→ (:Concept)
```

---

## 3. Backend Python — All Neo4j Code

### 3.1 Connection Initialization

**File: `core/synthetic_mind.py` (line 56–62)**
```python
from neo4j import GraphDatabase

# Inside SyntheticMind.__init__()
try:
    self.neo4j_driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "pci_secret"))
except Exception as e:
    print(f"Warning: Failed to connect to Neo4j - {e}")
    self.neo4j_driver = None
```

**File: `subconscious/tasks.py` (line 35–59) — `get_db_clients()`**
```python
def get_db_clients():
    """Initialise all DB/inference clients. Returns (neo4j_driver, collection, ollama_client)."""
    neo4j_driver = None
    try:
        neo4j_driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "pci_secret"))
    except Exception as e:
        print(f"[Subconscious] Neo4j connection failed: {e}")
    # ... also initializes ChromaDB + Ollama ...
    return neo4j_driver, collection, ollama_client
```

---

### 3.2 `sync_to_graph()` — Entity/Concept/Event Writing

**File: `core/synthetic_mind.py` (line 139–185)**

Called after every message to populate the knowledge graph with extracted entities + VBSE chemistry.

```python
def sync_to_graph(self, user_id, memory_id, extracted, context_text=""):
    if not self.neo4j_driver: return
    from memory.physics import calculate_vbse_properties

    with self.neo4j_driver.session() as session:
        # Core nodes
        session.run("MERGE (u:User {id: $user_id})", user_id=user_id)
        session.run("MERGE (m:Memory {id: $memory_id})", memory_id=memory_id)

        # Concepts with VBSE chemistry
        for c in extracted.get('concepts', []):
            if not c: continue
            chem = calculate_vbse_properties(c, context_text)
            session.run("""
                MERGE (u:User {id: $user_id})
                MERGE (c:Concept {name: $name})
                ON CREATE SET c.valence=$valence, c.electronegativity=$electronegativity,
                              c.phase_state=$phase_state, c.health=1.0
                MERGE (m:Memory {id: $memory_id})
                MERGE (u)-[:INTERESTED_IN]->(c)
                MERGE (m)-[:ABOUT]->(c)
            """, user_id=user_id, name=c, memory_id=memory_id, **chem)

        # Entities → User relationship
        for en in extracted.get('entities', []):
            if not en: continue
            session.run("""
                MERGE (u:User {id: $user_id})
                MERGE (en:Entity {name: $en_name})
                MERGE (u)-[:INTERACTS_WITH]->(en)
            """, user_id=user_id, en_name=en)

        # Events → Entity relationships
        for ev in extracted.get('events', []):
            if not ev: continue
            session.run("MERGE (e:Event {name: $name})", name=ev)
            for en in extracted.get('entities', []):
                if not en: continue
                session.run("""
                    MERGE (ev:Event {name: $ev_name})
                    MERGE (en:Entity {name: $en_name})
                    MERGE (ev)-[:INVOLVES]->(en)
                """, ev_name=ev, en_name=en)
```

---

### 3.3 Hybrid Retrieval — Symbolic Facts from Neo4j

**File: `core/synthetic_mind.py` (line 293–329) — `get_hybrid_context()`**

Fuses ChromaDB neural memory with Neo4j symbolic facts for context injection.

```python
# Inside get_hybrid_context():
symbolic_facts = []
if self.neo4j_driver:
    extracted_list = (extracted_entities_dict.get('concepts', []) +
                      extracted_entities_dict.get('entities', []) +
                      extracted_entities_dict.get('events', []))
    with self.neo4j_driver.session() as session:
        for entity in extracted_list:
            if not entity: continue
            query = """
            MATCH (n {name: $entity})-[r]-(related)
            RETURN n.name + ' ' + type(r) + ' ' + coalesce(related.name, related.id) as fact
            LIMIT 10
            """
            try:
                result = session.run(query, entity=entity)
                symbolic_facts.extend([record["fact"] for record in result])
            except Exception as e:
                print(f"Cypher Error: {e}")

fused_context = f"EPISODIC MEMORIES:\n{memory_texts}\n\nFACTUAL KNOWLEDGE:\n{symbolic_facts}"
```

---

### 3.4 CG Service — Contradiction Graph

**File: `memory/cg_service.py` (96 lines)**

Writes `[:CONTRADICTS]` edges between conflicting memories.

```python
def log_contradiction_edge(mem_id_a, mem_id_b, neo4j_driver):
    with neo4j_driver.session() as session:
        session.run("""
            MERGE (m1:Memory {id: $id_a})
            MERGE (m2:Memory {id: $id_b})
            MERGE (m1)-[r:CONTRADICTS]->(m2)
            SET r.timestamp = timestamp()
        """, id_a=mem_id_a, id_b=mem_id_b)
```

**Trigger**: Called from `save_memory()` after every ChromaDB write if distance < 0.7 and LLM confirms contradiction.

---

### 3.5 HIS Service — Hallucination Immune System (Neo4j Verification)

**File: `memory/his_service.py` (line 43–86) — `verify_claims()`**

Falls back to Neo4j keyword search if ChromaDB can't verify a claim.

```python
# Inside verify_claims():
if not is_verified and neo4j_driver:
    keywords = [w for w in claim.split() if len(w) > 4]
    if keywords:
        with neo4j_driver.session() as session:
            for kw in keywords:
                res = session.run(
                    "MATCH (n) WHERE toLower(n.name) CONTAINS toLower($kw) RETURN n LIMIT 1",
                    kw=kw
                )
                if res.single():
                    is_verified = True
                    break
```

---

### 3.6 MUR Service — Meta-Uncertainty Reflector (LacunaNode)

**File: `memory/mur_service.py` (109 lines)**

Three Neo4j operations:

**CREATE LacunaNode:**
```python
def detect_and_log_lacuna(user_query, response_text, retrieved_memory_count, has_uncertain_memories, neo4j_driver):
    # ...detection logic...
    with neo4j_driver.session() as session:
        session.run("""
            MERGE (u:User {id: 'user_1'})
            CREATE (l:LacunaNode {
                id: $lacuna_id, topic: $topic, confidence: $confidence,
                timestamp: $timestamp, resolved: false
            })
            MERGE (u)-[:HAS_KNOWLEDGE_GAP]->(l)
        """, lacuna_id=lacuna_id, topic=user_query, confidence=0.0, timestamp=...)
```

**READ Unresolved Lacunas:**
```python
def get_unresolved_lacunas(neo4j_driver, limit=3):
    with neo4j_driver.session() as session:
        result = session.run("""
            MATCH (l:LacunaNode {resolved: false})
            RETURN l.id AS id, l.topic AS topic
            ORDER BY l.timestamp DESC LIMIT $limit
        """, limit=limit)
        return [{"id": r["id"], "topic": r["topic"]} for r in result]
```

**RESOLVE Lacuna:**
```python
def mark_lacuna_resolved(lacuna_id, neo4j_driver):
    with neo4j_driver.session() as session:
        session.run("""
            MATCH (l:LacunaNode {id: $id})
            SET l.resolved = true, l.resolved_at = $timestamp
        """, id=lacuna_id, timestamp=...)
```

---

### 3.7 VBSE Physics Engine — Chemical Properties for Graph Nodes

**File: `memory/physics.py` (68 lines)**

Computes properties attached to `Concept` nodes in Neo4j:

| Property | Range | Logic |
|---|---|---|
| `valence` | 1–4 | `min(4, max(1, word_count // 10))` — bondable slots |
| `electronegativity` | 0.0–1.0 | `0.3 + (pull_word_count × 0.15)` — semantic pull |
| `phase_state` | 0.3 or 0.85 | 0.3 if speculative, 0.85 if factual |

**Activation Energy** (for Dream Weaver bonding):
```
E_ij = semantic_distance × exp(-(valence_A × 0.25 + electronegativity_B))
```

---

## 4. Subconscious Celery Tasks — Neo4j Operations

### 4.1 Dream Weaver (`subconscious/tasks.py`, line 109–214)

Runs every **120 seconds**. Creates `[:SYNAPSE]` edges between unconnected concepts.

**Cypher — Fetch unconnected pairs:**
```cypher
MATCH (a:Concept), (b:Concept)
WHERE a <> b AND NOT (a)-[:SYNAPSE]-(b)
  AND a.valence IS NOT NULL AND b.valence IS NOT NULL
RETURN a.name AS name_a, a.valence AS val_a, a.electronegativity AS en_a, a.health AS health_a,
       b.name AS name_b, b.valence AS val_b, b.electronegativity AS en_b, b.health AS health_b
LIMIT 1
```

**Cypher — Create synapse (if temperature > activation energy):**
```cypher
MATCH (a:Concept {name: $name_a}), (b:Concept {name: $name_b})
MERGE (a)-[r:SYNAPSE]->(b)
SET r.weight = $weight, r.activation_energy = $energy, r.timestamp = $timestamp
```

**Cypher — Degrade health on failed reaction:**
```cypher
MATCH (c:Concept {name: $name})
SET c.health = CASE WHEN c.health > 0.1 THEN c.health - 0.05 ELSE 0.1 END
```

**Cypher — LLM fallback (no VBSE nodes):**
```cypher
MERGE (u:User {id: 'user_1'})
MERGE (c:Concept {name: 'Dream Insight'})
MERGE (u)-[r:SYNAPSE {insight: $insight, timestamp: $timestamp}]-(c)
```

### 4.2 Theory of Mind Audit (`subconscious/tasks.py`, line 216–247)

Runs every **10 minutes**. Updates user psychological profile.

```cypher
MERGE (u:User {id: 'user_1'})
SET u.psychological_profile = $profile
```

### 4.3 Proactive Pulse Check (`subconscious/tasks.py`, line 249–316)

Runs every **1 hour**. Reads MUR Lacunas, researches them, marks resolved.

Uses `get_unresolved_lacunas()` and `mark_lacuna_resolved()` from MUR service (see §3.6).

---

## 5. Frontend — Neo4j Integration

### 5.1 Next.js API Route (`frontend/src/app/api/neo4j/route.ts`, 85 lines)

Server-side route that queries Neo4j directly from the frontend.

```typescript
import neo4j from 'neo4j-driver';

const URI  = process.env.NEO4J_URI  || 'bolt://localhost:7687';
const USER = process.env.NEO4J_USER || 'neo4j';
const PASS = process.env.NEO4J_PASSWORD || 'pci_secret';

export async function GET() {
  const driver = neo4j.driver(URI, neo4j.auth.basic(USER, PASS));
  const session = driver.session();
  // ...queries...
}
```

**Cypher queries executed:**

| Query | Returns |
|---|---|
| `MATCH (n) OPTIONAL MATCH (n)-[r]->() RETURN count(DISTINCT n) AS nodes, count(r) AS edges` | Total node & edge counts |
| `MATCH (l:LacunaNode) RETURN l.topic, l.confidence, l.resolved, l.timestamp ORDER BY l.timestamp DESC LIMIT 20` | Knowledge gaps |
| `MATCH ()-[r:SYNAPSE]->() RETURN r.insight, r.timestamp ORDER BY r.timestamp DESC LIMIT 10` | Dream Weaver synapses |
| `MATCH (u:User)-[:INTERESTED_IN]->(c:Concept) RETURN c.name LIMIT 20` | Hot concepts |
| `MATCH (e:Entity)-[:INTERACTS_WITH]->(u:User) RETURN e.name LIMIT 20` | Known entities |
| `MATCH ()-[r:CONTRADICTS]->() RETURN count(r) AS total` | Contradiction count |

**Response shape:**
```json
{
  "nodes": 42,
  "edges": 87,
  "lacunas": [...],
  "lacuna_count": 3,
  "synapses": [{"insight": "...", "ts": "..."}],
  "concepts": ["AI", "Rust", ...],
  "entities": ["Gaurang", "Mumbai", ...],
  "contradictions": 2,
  "timestamp": 1714987200000
}
```

### 5.2 Dashboard Page (`frontend/src/app/dashboard/page.tsx`)

Polls `/api/neo4j` every **3 seconds** and renders:
- **Graph Topology** panel: node count, edge count, contradiction count, synapse count
- **Hot Concepts**: tag cloud of `[:INTERESTED_IN]` concepts
- **Macro-Uncertainties**: active Lacuna count + topic list
- **Dream Weaver Synapses**: grid of subconscious insight cards

### 5.3 Chat Page (`frontend/src/app/page.tsx`)

Line 206 — displays "Neo4j Synced" status indicator in sidebar.
Line 403 — describes dynamic Neo4j ontology generation.

### 5.4 Frontend package.json

```json
"neo4j-driver": "^6.0.1"
```

---

## 6. FastAPI Endpoint — Graph Sync

**File: `api/routes.py` (line 170–178)**

```python
@router.post("/graph/sync")
async def sync_knowledge_graph():
    """Endpoint for syncing background structural updates into the Neo4j Knowledge Graph."""
    return {
        "status": "success",
        "message": "Symbolic Knowledge Graph synchronized with recent subconscious inferences."
    }
```

> **Note**: This is currently a stub/placeholder endpoint. Actual graph syncing happens automatically via `sync_to_graph()` during every chat message.

---

## 7. Data Flow — Where Neo4j Is Used in the Message Lifecycle

```
User sends message
  ↓
1. extract_entities() → {concepts, entities, events}
  ↓
2. get_hybrid_context()
   ├─ ChromaDB vector search (neural)
   └─ Neo4j Cypher queries (symbolic facts)  ◄── READ
  ↓
3. generate_response() → reply sent to user
  ↓ (async background)
4. save_memory()
   ├─ ChromaDB write
   ├─ sync_to_graph()                        ◄── WRITE (entities/concepts/events)
   └─ scan_and_log_contradictions()
       └─ log_contradiction_edge()           ◄── WRITE ([:CONTRADICTS])
  ↓
5. run_immune_audit_background()
   ├─ HIS verify_claims()                    ◄── READ (keyword search)
   └─ MUR detect_and_log_lacuna()            ◄── WRITE (LacunaNode)
  ↓ (every 2 min)
6. dream_weaver()                            ◄── READ+WRITE ([:SYNAPSE])
  ↓ (every 10 min)
7. theory_of_mind_audit()                    ◄── WRITE (User.psychological_profile)
  ↓ (every 1 hr)
8. proactive_pulse_check()                   ◄── READ+WRITE (Lacunas)
```

---

## 8. Known Issues (from `backend.log`)

Recurring `CypherTypeError` when entity extraction returns nested objects instead of primitive strings:

```
neo4j.exceptions.CypherTypeError: Property values can only be of primitive types or arrays thereof.
Encountered: Map{name -> String("Machine Learning"), company -> String("Google"), ...}
```

**Root cause**: The Ollama LLM sometimes returns structured objects instead of flat strings in `extract_entities()`. These objects are then passed as Cypher parameters where only primitives are accepted.

**Affected operations**: `sync_to_graph()` — concept/entity/event MERGE statements.

**Fix needed**: Flatten or stringify nested objects before passing to Neo4j session.run().

---

## 9. Complete File Index — All Files Touching Neo4j

| File | Role | Lines | Key Functions |
|---|---|---|---|
| `core/synthetic_mind.py` | Brain orchestrator | 603 | `sync_to_graph()`, `get_hybrid_context()`, `save_memory()`, `run_immune_audit_background()` |
| `memory/cg_service.py` | Contradiction Graph | 96 | `log_contradiction_edge()`, `scan_and_log_contradictions()` |
| `memory/his_service.py` | Hallucination Immune | 115 | `verify_claims()` |
| `memory/mur_service.py` | Meta-Uncertainty | 109 | `detect_and_log_lacuna()`, `get_unresolved_lacunas()`, `mark_lacuna_resolved()` |
| `memory/physics.py` | VBSE Chemistry | 68 | `calculate_vbse_properties()`, `calculate_activation_energy()` |
| `subconscious/tasks.py` | Celery background | 317 | `dream_weaver()`, `theory_of_mind_audit()`, `proactive_pulse_check()`, `get_db_clients()` |
| `api/routes.py` | FastAPI endpoints | 254 | `sync_knowledge_graph()` |
| `docker-compose.yml` | Infrastructure | 60 | Neo4j service definition |
| `requirements.txt` | Dependencies | 16 | `neo4j==5.19.0` |
| `frontend/.env.local` | Frontend config | 3 | `NEO4J_URI`, `NEO4J_USER`, `NEO4J_PASSWORD` |
| `frontend/package.json` | Frontend deps | — | `neo4j-driver: ^6.0.1` |
| `frontend/src/app/api/neo4j/route.ts` | Next.js API | 85 | Graph stats endpoint |
| `frontend/src/app/dashboard/page.tsx` | Dashboard UI | 669 | Polls `/api/neo4j`, renders graph topology |
| `frontend/src/app/page.tsx` | Chat UI | 411 | "Neo4j Synced" indicator |

---

*Generated by full project scan on 2026-05-06.*

