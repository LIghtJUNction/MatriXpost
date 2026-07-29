---
name: operating-intelligence
description: This skill should be used when the user asks to "manage a business object lifecycle", "record an operating ledger", "attribute content to a deal", "operate used-car inventory with MCP", "use lifecycle CLI commands", or "transition an operating object safely".
version: 0.1.0
---

# Operating Intelligence Lifecycle

Operate generic business lifecycle records through MatriXpost MCP and CLI. Model a vehicle as one template among many, not as a special core type. Keep this skill focused on local business records; do not build, invoke, or describe an agent runtime, publish content, dispatch a provider, or handle secrets.

## Start from the operating object

Choose a caller-owned stable ID and a generic `kind`, such as `vehicle`, `asset`, `project`, or `product`. Add `externalId` only when an outside identifier exists. For a used-car template, an external ID may hold a VIN, but never make VIN mandatory or encode it into generic workflows.

Create the object before recording cost, revenue, reimbursement, preparation, sale, receivable, aftersales, or content attribution. Use attributes only for safe descriptive fields. Do not put secrets in attributes: sensitive key names such as token, cookie, password, and session are rejected. Do not scan or reject ordinary business wording merely because it contains one of those words.

## Use the safe lifecycle order

1. Create or retrieve the object.
2. Append each expense or revenue event as a new immutable ledger entry.
3. Link relevant existing publication-history records to the object.
4. Read the current object revision.
5. Transition lifecycle and approval status with that exact revision.
6. Reload after a successful transition or a stale-revision rejection.

Treat `draft`, `active`, `completed`, and `archived` as controlled lifecycle values. Treat `pending`, `approved`, and `rejected` as approval values. Do not retry a revision conflict with the old revision. Do not mutate or delete ledger history to correct a business event; append a correcting entry through the approved business process.

## Use MCP for structured clients

Start `matrixpost-mcp` with a local state path when necessary. Keep MCP stdout as JSON-RPC and diagnostics on stderr. Use the eight lifecycle tools only:

- `list_business_objects`, `get_business_object`, `create_business_object`
- `list_ledger_entries`, `append_ledger_entry`
- `list_content_attributions`, `add_content_attribution`
- `transition_business_object`

Use snake_case tool names and camelCase JSON input fields. Treat returned core records as snake_case JSON. Read `references/mcp-cli-workflows.md` before composing tool inputs. Reject unknown fields rather than assuming they are ignored.

Treat a `not_found` result as missing parent state, not as an empty collection. Treat an empty list as valid only after confirming that the object exists and has no ledger entries or content links. Link attribution only after both the business object and history record exist.

## Use CLI for deterministic local operations

Pass `--state-path` before `lifecycle` to select an explicit SQLite database. Use `matrixpost lifecycle objects` to list, `matrixpost lifecycle object get` to read, and the object/ledger/attribution/transition commands in the reference for writes. Parse the JSON result and preserve returned revisions.

Use integer `--amount-minor` values with a three-letter ISO currency. Record an individual cost, reimbursement, sale, payment, or aftersales event as one append operation. Do not use a floating-point amount or a spreadsheet-style aggregate overwrite.

## Preserve permissions and boundaries

Apply least privilege at the caller boundary. Project a salesperson's assigned objects, customers, tasks, and permitted sale data; restrict procurement cost, profit, payment, and organization-wide financial data to authorized roles. Keep supplier, customer, commission, CRM, and content-ROI policies outside generic core until their explicit data contracts exist.

Use content links for attribution, not proof of ROI or a completed sale. Compute profitability, commissions, matching, dashboard metrics, and role-specific views only from explicitly implemented data and policy. Do not invent missing data, approval authority, remote publication outcomes, or AI/agent behavior.

## Reference

Load **`references/mcp-cli-workflows.md`** for exact MCP payloads and CLI commands. Keep examples generic and local. Update the reference when source-level tool or command names change.
