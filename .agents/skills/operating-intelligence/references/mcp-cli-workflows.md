# MCP and CLI lifecycle workflows

Use one local SQLite state store per workflow. Replace angle-bracket placeholders with safe business values. These examples manage local lifecycle state only; they do not publish content, run a browser, call a provider, read secrets, or create an agent.

## MCP workflow

Call `create_business_object` with camelCase input fields. Read the returned business object with its snake_case core-record fields, including `external_id`, `lifecycle_status`, `approval_status`, and `revision`.

```json
{
  "id": "asset-001",
  "kind": "vehicle",
  "displayName": "Delivery asset",
  "externalId": "external-reference-001",
  "lifecycleStatus": "draft",
  "approvalStatus": "pending",
  "attributes": {"brand": "Example", "usage": "delivery"}
}
```

Treat the returned `revision` as the next transition precondition. Append a cost with `append_ledger_entry`:

```json
{
  "id": "ledger-001",
  "businessObjectId": "asset-001",
  "direction": "expense",
  "category": "preparation",
  "amountMinor": 350000,
  "currency": "CNY",
  "approvalStatus": "pending",
  "description": "local preparation cost"
}
```

Call `list_ledger_entries` with `{"businessObjectId":"asset-001"}`. It returns `not_found` when the object does not exist and an empty list only when the object exists without entries.

Call `add_content_attribution` only after a history record exists:

```json
{
  "businessObjectId": "asset-001",
  "historyId": "history-001"
}
```

Transition with the precise returned revision:

```json
{
  "id": "asset-001",
  "expectedRevision": 0,
  "lifecycleStatus": "active",
  "approvalStatus": "approved"
}
```

On a stale-revision failure, call `get_business_object`, inspect the current object, and decide whether a new authorized transition is appropriate. Never retry automatically with an old revision.

## CLI workflow

Create the same object:

```sh
matrixpost --state-path ./operations.db lifecycle object create \
  --id asset-001 --kind vehicle --display-name 'Delivery asset' \
  --external-id external-reference-001 --attribute brand=Example --attribute usage=delivery
```

Append an immutable cost and inspect it:

```sh
matrixpost --state-path ./operations.db lifecycle ledger add \
  --id ledger-001 --object asset-001 --direction expense \
  --category preparation --amount-minor 350000 --currency CNY \
  --description 'local preparation cost'

matrixpost --state-path ./operations.db lifecycle ledger list --object asset-001
```

Link existing local publication history and make a revision-guarded transition:

```sh
matrixpost --state-path ./operations.db lifecycle attribution add \
  --object asset-001 --history history-001

matrixpost --state-path ./operations.db lifecycle transition \
  --id asset-001 --expected-revision 0 \
  --lifecycle-status active --approval-status approved
```

Read the returned JSON after every write. Use `matrixpost --state-path ./operations.db lifecycle object get --id asset-001` to obtain the current revision before a later transition.
