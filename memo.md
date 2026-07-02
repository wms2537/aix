# Vision Memo: The Versioned Computational Workspace

## 1. Executive Thesis

The product is not an “AI Excel clone,” a spreadsheet database, or a CLI wrapper around `.xlsx` files.

It is a new computational workspace designed around three realities:

1. **Excel remains the dominant universal computation and modeling format.**
2. **Lark Base-style multidimensional tables are a better model for operational data, relationships, workflows, and views.**
3. **AI agents require deterministic, structured, auditable execution surfaces rather than visual UI automation or unrestricted file editing.**

The long-term vision is:

> Build a local-first, Excel-compatible, Base-native, agent-operated computational workspace where data, formulas, views, charts, workflows, and agent actions are versioned, queryable, reproducible, and interoperable with XLSX.

The system should preserve the expressive freedom of Excel while fixing its structural weaknesses:

* data duplicated across sheets;
* formulas mixed with source data and presentation;
* records identified only by row position;
* weak auditability;
* fragile lookups;
* unclear ownership of truth;
* poor support for concurrent edits;
* no native agent transaction model;
* no reliable explanation of why a value changed.

At the same time, it should preserve what makes Excel indispensable:

* arbitrary grid-based modeling;
* dense financial and planning workbooks;
* formulas with relative and absolute references;
* rich formatting and layouts;
* familiar `.xlsx` interchange;
* widespread user adoption;
* support for ad hoc analysis that does not fit a rigid database schema.

The product should therefore combine:

```text
Excel:
  Freeform models, formulas, layout, compatibility

Lark Base:
  Typed records, relations, views, forms, workflows, permissions

Agent-native runtime:
  CLI, Skills, plans, dry-runs, proofs, revisions, audit trails

Analytical engine:
  SQL, vectorized computation, large datasets, materialized views
```

---

# 2. The Core Product Definition

The product is best described as:

> A versioned computational workspace with spreadsheet, database, and analytical semantics.

A workspace contains:

```text
Workspace
├─ Workbooks
├─ Sheets
├─ Tables
├─ Records
├─ Fields
├─ Relations
├─ Grid formulas
├─ Relational formulas
├─ Named queries
├─ Data Zones
├─ Views
├─ Charts
├─ Dashboards
├─ Forms
├─ Workflows
├─ Policies
├─ Revisions
└─ Agent receipts
```

The central idea is that a user should be able to work in whichever representation fits the task:

```text
Freeform financial model
→ Excel-style sheet and formulas

CRM or project tracker
→ Typed table, records, relations, Kanban, calendar, forms

Large sales dataset
→ Analytical table, SQL, chart, dashboard

Agent workflow
→ Structured plan, transactional patch, validation, export
```

All of these are part of one workspace rather than separate disconnected products.

---

# 3. The Two North Stars

## 3.1 Excel: computational compatibility and expressive freedom

Excel is the north star for:

* formula breadth;
* grid semantics;
* relative and absolute references;
* financial modeling;
* what-if planning;
* workbook structure;
* formatting;
* charts;
* named ranges;
* tables;
* spreadsheet interoperability;
* import and export fidelity.

The eventual ambition is not merely to support common formulas.

It is to build an independent Excel-compatible runtime with increasingly high practical compatibility.

That requires support for:

```text
Formula parsing
Dependency tracking
Recalculation
Dynamic arrays
Structured references
Named ranges
Date systems
Error propagation
Type coercion
Circular references
Iterative calculation
Volatile functions
Locale behavior
Formatting semantics
Workbook import/export
```

The practical principle is:

> No silent incorrectness.

When the runtime cannot reliably evaluate or preserve a workbook feature, it must report that condition explicitly rather than silently emitting plausible but wrong results.

## 3.2 Lark Base: operational data and multidimensional views

Lark Base is the north star for how operational information should behave.

Its important ideas are:

* records have stable identity;
* fields are typed;
* tables can be related;
* one dataset can have multiple views;
* forms feed directly into structured data;
* dashboards are first-class;
* workflows are attached to data events;
* permissions can be scoped semantically;
* a grid is only one representation of data.

The product should inherit this model:

```text
Orders table
├─ Grid view
├─ Kanban view grouped by status
├─ Calendar view by delivery date
├─ Form for order entry
├─ Dashboard for revenue and fulfillment
└─ Data Zone rendered into an Excel-style sheet
```

The same underlying records should power every representation.

No duplicated sheets. No copied data exports. No manually synchronized “dashboard tabs.”

---

# 4. The Fundamental Design Principles

## Principle 1: The grid is not always the source of truth

An Excel sheet is useful, but it should not be forced to become the storage model for every kind of data.

A workbook may contain:

```text
Workbook-authoritative areas
  Freeform cells, formulas, layouts, assumptions

Table-authoritative areas
  Typed records, relations, workflows, views

Query-authoritative areas
  Derived analytical results and materialized reports
```

The product must make authority explicit.

A range cannot simultaneously be:

```text
A manually editable Excel model
and
a live projection of a relational table
```

unless the synchronization rules are explicitly defined.

## Principle 2: Identity and location are different concepts

Excel treats location as identity:

```text
Forecast!B7
```

A Base-style system treats record identity as independent of display position:

```text
Orders record ord_932
```

The product needs both.

```text
Cell identity:
  sheet_id + row + column

Record identity:
  table_id + record_id

Field identity:
  table_id + field_id

Formula identity:
  formula_id
```

Rows may move. Records remain stable.

Cells may move. Their formulas must preserve relative and absolute reference semantics.

## Principle 3: Data, computation, and presentation must be separable

Traditional spreadsheets blur these together:

```text
Raw data
+ formulas
+ formatting
+ chart data
+ notes
+ workflow status
+ user input
```

The native model separates them:

```text
Data:
  tables, records, fields, attachments

Computation:
  formulas, query plans, materialized views

Presentation:
  sheets, views, charts, dashboards, forms

Control:
  policies, workflows, revisions, approvals
```

This separation is what allows agents to reason safely.

## Principle 4: Every meaningful change is a versioned transaction

A direct edit should be represented as a typed operation against a known revision.

```text
Read revision 42
→ propose patch
→ validate preconditions
→ simulate impact
→ apply atomically
→ calculate
→ verify outputs
→ create revision 43
```

The source of truth is not the latest mutable file state.

It is a sequence of durable workspace revisions.

## Principle 5: SQL is an interface, not the source of truth

SQL is valuable because it gives users and agents a familiar language for:

* filtering;
* joins;
* grouping;
* aggregations;
* inspection;
* materialized views;
* chart data;
* operational reporting.

But SQL should not define the core object model.

The source of truth is a versioned workspace graph of objects, chunks, manifests, and revisions.

## Principle 6: AI should enrich and operate the system, not make it non-deterministic

AI must not be embedded as invisible volatile formulas.

Bad:

```text
=AI("Predict next quarter revenue")
```

Better:

```text
AI enrichment job
├─ input dataset and row IDs
├─ model and version
├─ prompt template version
├─ temperature and parameters
├─ input hash
├─ output schema
├─ validation policy
├─ approval status
└─ lineage
```

AI-generated outputs should always be inspectable, rerunnable, invalidatable, and distinguishable from deterministic calculations.

---

# 5. The Native Storage Engine

The engine should be designed from first principles around access patterns rather than choosing SQLite, Parquet, or a database first.

The canonical storage engine is:

> A local append-only, copy-on-write workspace store built from immutable manifests and typed chunks.

It should support three fundamentally different physical data shapes.

```text
1. Sparse grid tiles
   For Excel-like sheets, formulas, styles, comments, validations, and layout.

2. Versioned record fragments
   For Base-like tables, typed fields, record-level updates, relations, and workflows.

3. Columnar analytical segments
   For large datasets, scans, joins, aggregations, dashboards, and vectorized computation.
```

These should share one logical workspace and one revision model.

---

## 5.1 Workspace structure

A native workspace may be represented as a portable directory during development and optionally packed into a single archive for distribution.

```text
workspace/
├─ refs/
│  ├─ HEAD
│  ├─ main
│  └─ agent/session-forecast-review
│
├─ commits/
│  ├─ commit_001
│  ├─ commit_002
│  └─ commit_003
│
├─ manifests/
│  ├─ workspace-manifest
│  ├─ workbook-manifest
│  ├─ sheet-manifest
│  ├─ table-manifest
│  ├─ query-manifest
│  └─ view-manifest
│
├─ chunks/
│  ├─ grid-tiles/
│  ├─ table-fragments/
│  ├─ table-deltas/
│  ├─ columnar-segments/
│  ├─ formula-asts/
│  ├─ style-dictionaries/
│  └─ chart-specs/
│
├─ blobs/
│  ├─ images/
│  ├─ attachments/
│  ├─ original-xlsx/
│  ├─ VBA-projects/
│  └─ opaque-ooxml-parts/
│
└─ indexes/
   ├─ catalog-index
   ├─ record-index
   ├─ dependency-index
   ├─ search-index
   └─ statistics-index
```

The distinction is important:

```text
Canonical:
  commits, manifests, chunks, blobs

Derived and rebuildable:
  indexes, caches, temporary query files
```

If an index becomes corrupted, it can be rebuilt.

If a cache becomes invalid, it can be recalculated.

The workspace history and chunk graph remain authoritative.

---

## 5.2 Sparse grid tiles

A sheet should not store every cell as a separate database row.

This is inefficient for large sparse sheets and weak for range-oriented operations.

Instead, divide sheets into tiles.

```text
Sheet: Forecast

Tile 0,0
A1:CV128

Tile 0,1
CW1:GR128

Tile 1,0
A129:CV256
```

A tile stores only populated cells and supporting metadata.

```text
Grid Tile
├─ cell offsets
├─ literal values
├─ formula IDs
├─ style IDs
├─ comments
├─ validation references
├─ row/column overrides
└─ local metadata
```

Changing one cell creates a new version of only the affected tile.

```text
Forecast!G12 changes
→ new grid tile
→ new sheet manifest
→ new workspace revision
```

Unchanged tiles are shared between revisions.

This enables:

* efficient range reads;
* cheap history;
* cell-level diffs;
* rollback;
* branch creation;
* agent simulation;
* fast save operations.

---

## 5.3 Versioned record fragments

Base-style tables require stable record IDs and typed fields.

```text
Orders
├─ order_id
├─ customer_id
├─ status
├─ order_date
├─ revenue
└─ cost
```

A record update should not rewrite an entire table.

```text
record_id: ord_932
field_id: status
old_value: Paid
new_value: Refunded
revision: 43
```

The storage model should support:

```text
Base table
├─ immutable base fragments
├─ append-only delta fragments
├─ record indexes
├─ relation indexes
└─ periodic compaction
```

This is optimized for operational workflows:

* updating one record;
* changing a status;
* assigning a task;
* adding a comment;
* submitting a form;
* creating a relation;
* applying permissions;
* triggering an automation.

---

## 5.4 Columnar analytical segments

Large datasets need a different physical layout.

```text
Orders segment
├─ record_id column
├─ customer_id column
├─ region column
├─ revenue column
├─ cost column
└─ order_date column
```

This supports analytical queries efficiently:

```sql
SELECT
  region,
  DATE_TRUNC('month', order_date) AS month,
  SUM(revenue) AS revenue,
  SUM(revenue - cost) AS gross_profit
FROM orders
GROUP BY 1, 2;
```

The architecture should follow this pattern:

```text
Operational write
→ append row or field delta

Analytical read
→ scan compact columnar fragments

Compaction
→ merge base fragments and deltas into new optimized segments
```

This prevents the false choice between:

```text
Fast operational updates
versus
Fast analytical scans
```

The runtime needs both.

---

# 6. The Data Zone: The Bridge Between Excel and Base

The most important user-facing primitive is the Data Zone.

A Data Zone is a sheet region backed by a table or query rather than ordinary manually owned cells.

```text
Sheet: Executive Dashboard
Range: A5:H40
Source: monthly_revenue query
Authority: Orders and Customers tables
Mode: read-only live projection
Refresh: on source revision
```

The user sees a grid.

The engine sees a declarative projection.

```text
Table or query
→ Data Zone
→ Sheet range
→ Chart, dashboard, export, formula reference
```

A Data Zone may be:

```text
Read-only
  Used for dashboards, reports, query outputs

Write-through
  Used for editable table views with field validation

Snapshot
  Materialized at a specific revision for reporting or export
```

This solves the classic spreadsheet problem:

```text
Data copied into five sheets
→ formulas diverge
→ totals disagree
→ nobody knows which sheet is correct
```

A Data Zone preserves grid familiarity while keeping semantic authority in one place.

---

# 7. Three Computation Systems

The product needs three distinct calculation languages.

Trying to compress them into one language creates poor ergonomics and poor performance.

## 7.1 Excel Formula Mode

Used for imported workbooks and freeform models.

```excel
=SUM(B2:B20)
=XLOOKUP(A2, Orders[OrderID], Orders[Revenue])
=LET(x, B2:B10, SUM(x))
```

This mode must preserve Excel semantics:

```text
Relative references
Absolute references
Ranges
Named ranges
Structured references
Spill behavior
Error propagation
Date systems
Type coercion
Calculation order
```

Formula references must preserve their semantic form, not merely resolved coordinates.

```text
Reference
├─ sheet ID
├─ row mode: relative or absolute
├─ column mode: relative or absolute
└─ target coordinate
```

Otherwise copy, fill, move, insertion, and deletion behavior will be wrong.

## 7.2 Relational Formula Mode

Used for computed fields in Base-like tables.

```text
gross_margin = revenue - cost

is_overdue =
  due_date < today()
  AND status != "Done"

customer_lifetime_value =
  related_orders.sum(revenue)
```

This should compile into vectorized operations or query plans.

It must not degenerate into repeated row-by-row table scans.

## 7.3 Query Mode

Used for multi-table analysis, joins, aggregation, reporting, and materialization.

```sql
SELECT
  customer.region,
  SUM(order.revenue) AS total_revenue
FROM customers AS customer
JOIN orders AS order
  ON order.customer_id = customer.record_id
GROUP BY customer.region;
```

The three modes should share one dependency and lineage system, but remain distinct languages.

```text
Excel mode:
  Freeform grid compatibility

Relational mode:
  Typed row and relation logic

Query mode:
  Large-scale analytics and materialized views
```

---

# 8. Dependency Graph and Recalculation

The dependency graph must be more sophisticated than cell-to-cell edges.

A formula such as:

```excel
=SUM(A1:A1000000)
```

must not create one million dependency edges.

The runtime needs range and object-level dependency nodes.

```text
Dependency graph nodes
├─ Cell
├─ Range
├─ Sheet
├─ Named range
├─ Table field
├─ Record field
├─ Relation
├─ Query
├─ Data Zone
├─ Formula
├─ Chart
└─ External input
```

Example invalidation:

```text
Orders[revenue] changes
→ monthly_revenue query invalidates
→ Executive Dashboard Data Zone invalidates
→ monthly revenue chart invalidates
→ summary formula cells invalidate
```

Formula results are caches, not source data.

A calculated result should be keyed by:

```text
Formula hash
+ input revision
+ engine version
+ locale
+ date system
+ timezone
+ deterministic clock
+ random seed
```

This allows reproducible calculation and debugging.

---

# 9. Numeric and Type Semantics

The runtime must support different numeric domains.

```text
Excel Number
  For Excel-compatible floating-point behavior

Decimal
  For exact money, tax, accounting, and quantity logic

Integer
  For IDs, counters, indexes, and discrete values

Date and DateTime
  For semantic, timezone-aware values

Excel Date Serial
  For imported workbook compatibility
```

Do not silently infer that every number formatted as currency should become an exact decimal.

The Excel compatibility runtime and the typed-table runtime must have explicit conversion and coercion rules.

This is essential for accounting-grade calculations and reliable import/export behavior.

---

# 10. XLSX Interoperability Strategy

`.xlsx` is an essential boundary format, but it cannot be the native representation for the entire product.

XLSX cannot naturally express all of the native system’s concepts:

* persistent revision history;
* record identity;
* Base-style views;
* policies;
* workflows;
* agent receipts;
* query plans;
* custom lineage;
* native semantic relations.

Therefore:

> The native workspace is authoritative. XLSX is an import/export projection.

## 10.1 Import modes

### Preserve Mode

Used for existing Excel models.

```text
XLSX
→ sheets
→ cells
→ formulas
→ names
→ styles
→ tables
→ charts
→ validations
→ opaque Office parts
```

The workbook remains Excel-authoritative.

### Lift Mode

Used when an Excel workbook contains operational tables that should become structured data.

```text
Excel tables or coherent ranges
→ detected table candidates
→ inferred field types
→ proposed primary keys
→ proposed relations
→ Data Zones
→ user approval
```

Lift mode must never silently convert a financial model into a database.

It should be a deliberate semantic migration.

## 10.2 Unsupported features

The runtime should distinguish:

```text
Native execution
  Fully supported and editable

Pass-through preservation
  Retained during round-trip export but not natively executed

Blocked
  Explicitly unsupported or unsafe to execute
```

Examples:

```text
Native:
  values, formulas, styles, named ranges, tables, charts

Pass-through:
  VBA projects, Power Query, pivot caches, external connections

Blocked:
  macro execution, unsafe external refresh, unsupported active content
```

Exports must include a compatibility report.

---

# 11. Agent-Native Execution Model

The agent interface should not be “give the model access to the XLSX ZIP file.”

It should be:

> Give the agent typed, revision-aware operations through a CLI, Agent Skill, and eventually MCP.

The fundamental agent primitive is:

```text
Propose a typed change set against a known workspace revision.
```

Example:

```json
{
  "base_revision": "rev_42",
  "actor": "agent:forecast-review",
  "operations": [
    {
      "type": "set_cell_formula",
      "sheet": "Forecast",
      "cell": "G12",
      "formula": "=SUM(G2:G11)"
    },
    {
      "type": "update_record",
      "table": "Orders",
      "record_id": "ord_932",
      "set": {
        "status": "Refunded"
      }
    }
  ],
  "proofs_required": [
    "no_formula_errors",
    "Forecast!G12 is numeric",
    "monthly_revenue query succeeds"
  ]
}
```

The runtime should return:

```text
Dry-run result
├─ affected cells
├─ affected records
├─ invalidated formulas
├─ invalidated queries
├─ affected charts
├─ estimated compute cost
├─ policy violations
├─ merge conflicts
└─ expected proof results
```

Only then should the change be committed.

## 11.1 CLI principles

Every capability must be invokable through the CLI.

```bash
workspace inspect
sheet list
range get
range write
formula get
formula set
record create
record update
query run
view create
chart render
calc run
calc verify
xlsx import
xlsx export
changes plan
changes apply
changes rollback
```

All commands should support:

```text
--json
--if-revision
--dry-run
--idempotency-key
--actor
--change-note
```

`stdout` should return machine-readable data.

`stderr` should contain logs, progress, and diagnostics.

## 11.2 Agent Skill

The Agent Skill should teach agents:

```text
Inspect first
Read only relevant ranges
Use structured IDs where available
Propose patches before applying
Use dry-run for meaningful changes
Verify named outputs after calculation
Export only after proof checks pass
```

The Skill is guidance.

The runtime is the enforcement boundary.

---

# 12. User Experience Model

The UI should not be one monolithic spreadsheet screen.

The user should be able to move between modes.

```text
Grid Mode
  Excel-style sheets, formulas, formatting, manual models

Table Mode
  Typed records, linked fields, structured editing

View Mode
  Kanban, calendar, Gantt, gallery, form

Query Mode
  SQL, query plans, result previews, materialization

Dashboard Mode
  KPI cards, charts, pivots, filters, interactive reporting

Trace Mode
  “Why is this value 184,220?”
```

Trace Mode is especially important.

A user should be able to inspect a result:

```text
Value: 184,220
├─ Formula or query
├─ Source cells or records
├─ Upstream calculations
├─ Dataset revision
├─ Agent or user action that changed it
└─ Validation status
```

This is a major advantage over both conventional spreadsheets and AI-generated dashboards.

---

# 13. Visualisation Model

Charts should be versioned semantic objects, not screenshots.

```json
{
  "source": "monthly_revenue",
  "kind": "line",
  "x": "month",
  "y": "revenue",
  "series": "region",
  "title": "Monthly Revenue by Region"
}
```

A chart can then be:

```text
Rendered in the UI
Embedded in a dashboard
Rendered to SVG
Rendered to HTML
Exported to PNG
Mapped to an XLSX chart where possible
Linked to source lineage
```

Every chart should know which query, fields, filters, and revision created it.

---

# 14. Local-First Distribution

The runtime should feel closer to `npx` than a heavyweight enterprise install.

The primary goal is:

```text
One command
→ one native executable
→ no required server
→ no Python
→ no Java
→ no Excel installation
→ no Docker
```

Potential distribution channels:

```text
Native installer
GitHub release binary
Package manager package
Cargo binstall
npm launcher package
Homebrew package
Windows package manager
```

An npm package may act as a thin launcher that resolves and caches the correct platform-specific Rust executable.

The engine itself remains native Rust.

Default runtime behavior:

```text
Open workspace
→ acquire lock
→ execute operation
→ write revision
→ update indexes
→ return receipt
→ exit
```

No daemon should be required for CLI use.

A local daemon may later support:

* desktop UI;
* live collaboration;
* background compaction;
* workflow scheduling;
* long-running data sync.

---

# 15. Collaboration and Branching

The system should use branches and explicit merges before adopting CRDTs as a core model.

For example:

```text
Agent A:
  changes Forecast!B7 to 120,000

Agent B:
  changes Forecast!B7 to 140,000
```

There is no correct automatic merge.

The system should create a conflict object.

```text
Conflict
├─ target: Forecast!B7
├─ base value: 100,000
├─ branch A value: 120,000
├─ branch B value: 140,000
├─ actor metadata
└─ resolution status
```

Conflict granularity should depend on object type.

```text
Cells:
  sheet + row + column + property

Records:
  table + record + field

Views:
  view configuration field

Charts:
  chart configuration field

Schemas:
  field ID, relation ID, or constraint
```

CRDTs may later be useful for:

* comments;
* cursors;
* presence;
* simple text fields;
* collaborative editing indicators.

They should not define the computation model.

---

# 16. Security and Governance

The workspace should treat external execution as hostile by default.

```text
Macros:
  preserve, never execute by default

External workbook links:
  preserve, disable automatic refresh

External connections:
  require explicit policy approval

AI tools:
  require declared permissions and lineage

Sensitive data:
  support scoped access controls and audit trails
```

Policies should be semantic:

```text
Who can read this table?
Who can edit this field?
Can an agent update formulas?
Can an agent export a range?
Can this workflow call an external API?
Can a chart expose sensitive columns?
```

A cell lock is insufficient.

---

# 17. Market Positioning

The product should not be positioned as:

```text
“A better spreadsheet”
“AI inside Excel”
“Another Airtable”
“A CLI for XLSX files”
```

The positioning should be:

> A local-first computational workspace where Excel-compatible models, structured operational data, analytics, charts, and AI agents operate on one versioned source of truth.

The core value proposition is:

```text
Excel compatibility
+ Base-like structured operations
+ local analytical engine
+ agent-safe automation
+ reproducibility and auditability
```

---

# 18. Initial Target Use Cases

The first users should be people and teams who already suffer from Excel complexity but cannot abandon Excel.

Strong initial use cases:

```text
Financial planning and forecasting models
Operational reporting workbooks
Sales and inventory analysis
Budgeting and scenario planning
Multi-sheet management reports
Agent-assisted spreadsheet remediation
XLSX batch calculation and CSV export
Spreadsheet audit and verification
```

The initial wedge should be:

> Import an existing operational workbook, calculate it headlessly, expose it through CLI and agents, lift selected tables into structured data, create live views and dashboards, and export back to XLSX.

This gives immediate value without requiring users to abandon their current files.

---

# 19. Product Phases

## Phase 0: Runtime Foundation

Goal:

```text
Make XLSX workbooks inspectable, editable, calculable, and exportable through one local CLI.
```

Core capabilities:

```text
XLSX import
XLSX export
Range read/write
Formula read/write
Headless recalculation
CSV export
Compatibility reports
JSON CLI contracts
Agent Skill
Revision history
Dry-run patches
```

## Phase 1: High-Confidence Calculation

Goal:

```text
Become trustworthy for real workbook calculation.
```

Capabilities:

```text
Formula compatibility corpus
Dynamic arrays
Named ranges
Structured references
Calculation trace
Error diagnostics
Excel versus native verification
LibreOffice verification fallback
Deterministic volatile-function modes
```

## Phase 2: Base-Native Tables

Goal:

```text
Introduce structured tables without abandoning workbooks.
```

Capabilities:

```text
Typed tables
Stable records
Relations
Table formulas
Forms
Views
Data Zones
Lift-mode import
Record-level permissions
```

## Phase 3: Analytical Workspace

Goal:

```text
Scale beyond ordinary spreadsheet limits.
```

Capabilities:

```text
Columnar datasets
SQL query engine
Materialized views
Large CSV and Parquet import
Vectorized operations
Dashboard queries
Chart lineage
```

## Phase 4: Agentic Operations

Goal:

```text
Make agents reliable operators of complex workspaces.
```

Capabilities:

```text
Patch plans
Preconditions
Proof obligations
Policy checks
Branching
Merge conflicts
Rollback
Action receipts
MCP integration
```

## Phase 5: Collaborative Platform

Goal:

```text
Support teams, workflows, and distributed workspaces.
```

Capabilities:

```text
Remote sync
Collaboration
Approvals
Shared views
Scheduled workflows
Enterprise controls
Hosted or private deployment
```

---

# 20. What Not to Build First

Avoid these traps:

```text
Do not begin by recreating all of Excel’s UI.
Do not begin with a web spreadsheet editor.
Do not begin with real-time multiplayer collaboration.
Do not begin with Power Query or VBA execution.
Do not begin with every chart type.
Do not make AI chat the main interface.
Do not build a generic database and call it a spreadsheet.
Do not force all computation into SQL.
Do not force all data into cells.
Do not use agent-generated code as the source of truth.
```

The first product must prove that the core runtime is trustworthy.

---

# 21. The Long-Term Moat

The long-term moat is not merely formula support.

It is the combination of:

```text
Excel compatibility corpus
+ calculation correctness
+ XLSX round-trip fidelity
+ semantic data model
+ versioned workspace engine
+ agent transaction model
+ lineage and proof system
+ local-first distribution
```

The compatibility corpus becomes particularly important.

Every workbook should produce test assets such as:

```text
Input workbook
Expected calculated outputs
Expected errors
Expected export behavior
Expected rendering snapshots
Compatibility notes
```

Over time, the corpus becomes a defensible asset.

---

# 22. Final Product Statement

The final product should feel like this:

> A user can import an Excel workbook, retain its formulas and visual model, convert selected ranges into structured relational data, expose those records through multiple views and forms, query large datasets with SQL, let an AI agent safely inspect and modify the workspace through transactional tools, trace every number back to its origin, and export the result back into Excel when necessary.

That is the real vision.

Not Excel replaced by AI.

Not Lark Base with more formulas.

Not a database hidden behind a grid.

A new category:

> **The versioned computational workspace: Excel-compatible, Base-native, analytical by design, and safe for AI agents to operate.**
