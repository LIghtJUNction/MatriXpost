const tauriCore = window.__TAURI__?.core;
const bridgeError = typeof tauriCore?.invoke === "function"
  ? null
  : "The Tauri IPC bridge is unavailable. Start the installed MatriXpost desktop application and check its stderr output.";
const invoke = bridgeError
  ? () => Promise.reject(new Error(bridgeError))
  : tauriCore.invoke.bind(tauriCore);

const platforms = document.querySelector("#platforms");
const targets = document.querySelector("#target-options");
const summary = document.querySelector("#summary");
const accounts = document.querySelector("#accounts");
const result = document.querySelector("#draft-result");
const localRunnerTargets = document.querySelector("#local-runner-target-options");
const localRunnerResult = document.querySelector("#local-runner-result");
const localRunnerOutcomes = document.querySelector("#local-runner-outcomes");
const accountReadinessResult = document.querySelector("#account-readiness-result");
const fanqieReviewResult = document.querySelector("#fanqie-review-result");
const accountResult = document.querySelector("#account-result");
const articleAccounts = document.querySelector("#article-accounts");
const articleAccountResult = document.querySelector("#article-account-result");
const historyForm = document.querySelector("#history-form");
const history = document.querySelector("#history");
const historyResult = document.querySelector("#history-result");
const lifecycleObjectsList = document.querySelector("#lifecycle-objects");
const lifecycleObjectSelect = document.querySelector("#lifecycle-object-select");
const lifecycleObjectResult = document.querySelector("#lifecycle-object-result");
const lifecycleLedger = document.querySelector("#lifecycle-ledger");
const lifecycleLedgerResult = document.querySelector("#lifecycle-ledger-result");
const lifecycleAttributions = document.querySelector("#lifecycle-attributions");
const lifecycleAttributionResult = document.querySelector("#lifecycle-attribution-result");
const lifecycleRelations = document.querySelector("#lifecycle-relations");
const lifecycleRelationResult = document.querySelector("#lifecycle-relation-result");
const lifecycleRelationTargetSelect = document.querySelector("#lifecycle-relation-target-select");
const lifecycleTransitionResult = document.querySelector("#lifecycle-transition-result");
const lifecycleTransitionForm = document.querySelector("#lifecycle-transition-form");
let lifecycleObjects = [];

if (bridgeError) {
  document.querySelector("#availability").textContent = bridgeError;
  document.querySelectorAll("button, input, select, textarea").forEach((control) => {
    control.disabled = true;
  });
}

function platformLabel(platform) {
  return `${platform.name} (${platform.code})`;
}

function renderSnapshot(snapshot) {
  summary.replaceChildren(
    ...[
      ["Video accounts", snapshot.accounts.length],
      ["Article accounts", snapshot.article_accounts.length],
      ["History entries", snapshot.history_count],
    ].flatMap(([label, value]) => {
      const term = document.createElement("dt");
      term.textContent = label;
      const detail = document.createElement("dd");
      detail.textContent = value;
      return [term, detail];
    }),
  );

  platforms.replaceChildren(...snapshot.platforms.map((platform) => {
    const item = document.createElement("span");
    item.className = "platform unavailable";
    item.textContent = `${platformLabel(platform)} · unavailable`;
    return item;
  }));

  targets.replaceChildren(...snapshot.platforms.map((platform, index) => {
    const label = document.createElement("label");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.name = "targets";
    checkbox.value = platform.code;
    checkbox.checked = index === 0;
    label.append(checkbox, ` ${platformLabel(platform)}`);
    return label;
  }));

  localRunnerTargets.replaceChildren(...snapshot.platforms.map((platform, index) => {
    const label = document.createElement("label");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.name = "targets";
    checkbox.value = platform.code;
    checkbox.checked = index === 0;
    label.append(checkbox, ` ${platformLabel(platform)}`);
    return label;
  }));

  accounts.replaceChildren(...snapshot.accounts.map((account) => {
    const item = document.createElement("li");
    item.className = "account";
    const name = document.createElement("strong");
    name.textContent = account.display_name;
    const details = document.createElement("span");
    details.textContent = `${account.platform} · ${account.status}`;
    item.append(name, details);
    return item;
  }));

  if (snapshot.accounts.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty-state";
    empty.textContent = "No local video accounts saved.";
    accounts.append(empty);
  }

  articleAccounts.replaceChildren(...snapshot.article_accounts.map((account) => {
    const item = document.createElement("li");
    item.className = "account";
    const name = document.createElement("strong");
    name.textContent = account.display_name;
    const details = document.createElement("span");
    details.textContent = `Juejin · ${account.status}`;
    item.append(name, details);
    return item;
  }));

  if (snapshot.article_accounts.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty-state";
    empty.textContent = "No local Juejin article metadata saved.";
    articleAccounts.append(empty);
  }
}

function renderHistory(entries) {
  history.replaceChildren(...entries.map((entry) => {
    const item = document.createElement("li");
    item.className = "history-entry";
    const title = document.createElement("strong");
    title.textContent = entry.title;
    const details = document.createElement("span");
    const intent = entry.draft ? "draft" : (entry.scheduled ? "scheduled" : "immediate");
    details.textContent = `${entry.state} · ${entry.targets.join(", ")} · ${intent} · ${entry.recorded_at}`;
    item.append(title, details);
    return item;
  }));

  if (entries.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty-state";
    empty.textContent = "No local history matches these filters.";
    history.append(empty);
  }
}

function renderLocalRunnerOutcomes(outcomes) {
  localRunnerOutcomes.replaceChildren(...outcomes.map((outcome) => {
    const item = document.createElement("li");
    item.className = "account";
    const title = document.createElement("strong");
    title.textContent = `${outcome.platform} · ${outcome.state}`;
    const details = document.createElement("span");
    details.textContent = outcome.reason;
    item.append(title, details);
    return item;
  }));
}

async function refreshHistory() {
  const form = new FormData(historyForm);
  const all = form.get("all") === "on";
  try {
    const entries = await invoke("local_history", {
      input: {
        days: all ? null : Number(form.get("days")),
        all,
        platform: form.get("platform") || null,
        status: form.get("status") || null,
      },
    });
    renderHistory(entries);
    historyResult.textContent = entries.length === 0 ? "No matching local history." : "";
  } catch (error) {
    history.replaceChildren();
    historyResult.textContent = `Unable to read local history: ${String(error)}`;
  }
}

async function refresh() {
  try {
    renderSnapshot(await invoke("desktop_snapshot"));
    await refreshHistory();
    await refreshLifecycle();
  } catch (error) {
    result.textContent = `Unable to read local state: ${String(error)}`;
  }
}

function selectedLifecycleObject() {
  return lifecycleObjects.find((object) => object.id === lifecycleObjectSelect.value) || null;
}

function localId(prefix) {
  if (window.crypto && typeof window.crypto.randomUUID === "function") {
    return `${prefix}-${window.crypto.randomUUID()}`;
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function appendEmpty(list, message) {
  const item = document.createElement("li");
  item.className = "empty-state";
  item.textContent = message;
  list.append(item);
}

function renderLifecycleObjects(objects) {
  const previousId = lifecycleObjectSelect.value;
  lifecycleObjects = objects;
  lifecycleObjectSelect.replaceChildren();
  lifecycleObjectsList.replaceChildren(...objects.map((object) => {
    const item = document.createElement("li");
    item.className = "account";
    const title = document.createElement("strong");
    title.textContent = object.displayName;
    const details = document.createElement("span");
    const external = object.externalId ? ` · ${object.externalId}` : "";
    details.textContent = `${object.kind}${external} · ${object.lifecycleStatus} · ${object.approvalStatus} · revision ${object.revision}`;
    item.append(title, details);
    return item;
  }));

  if (objects.length === 0) {
    appendEmpty(lifecycleObjectsList, "No local lifecycle objects saved.");
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Create an object first";
    lifecycleObjectSelect.append(option);
    lifecycleObjectSelect.disabled = true;
    renderLifecycleRelationTargetOptions(null);
    return;
  }

  lifecycleObjectSelect.disabled = false;
  lifecycleObjectSelect.append(...objects.map((object) => {
    const option = document.createElement("option");
    option.value = object.id;
    option.textContent = `${object.displayName} (${object.kind})`;
    return option;
  }));
  lifecycleObjectSelect.value = objects.some((object) => object.id === previousId)
    ? previousId
    : objects[0].id;
  renderLifecycleRelationTargetOptions(lifecycleObjectSelect.value);
}

function renderLifecycleRelationTargetOptions(sourceObjectId) {
  const previousId = lifecycleRelationTargetSelect.value;
  const targetObjects = lifecycleObjects.filter((object) => object.id !== sourceObjectId);
  lifecycleRelationTargetSelect.replaceChildren();

  if (targetObjects.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Create another object to connect it";
    lifecycleRelationTargetSelect.append(option);
    lifecycleRelationTargetSelect.disabled = true;
    return;
  }

  lifecycleRelationTargetSelect.disabled = false;
  lifecycleRelationTargetSelect.append(...targetObjects.map((object) => {
    const option = document.createElement("option");
    option.value = object.id;
    option.textContent = `${object.displayName} (${object.kind}) · ${object.id}`;
    return option;
  }));
  lifecycleRelationTargetSelect.value = targetObjects.some((object) => object.id === previousId)
    ? previousId
    : targetObjects[0].id;
}

function renderLifecycleLedger(entries) {
  lifecycleLedger.replaceChildren(...entries.map((entry) => {
    const item = document.createElement("li");
    item.className = "account";
    const title = document.createElement("strong");
    title.textContent = `${entry.direction} · ${entry.amountMinor} ${entry.currency}`;
    const details = document.createElement("span");
    details.textContent = `${entry.category} · ${entry.approvalStatus} · ${entry.occurredAt}`;
    item.append(title, details);
    return item;
  }));
  if (entries.length === 0) {
    appendEmpty(lifecycleLedger, "No ledger entries for this object.");
  }
}

function renderLifecycleAttributions(entries) {
  lifecycleAttributions.replaceChildren(...entries.map((entry) => {
    const item = document.createElement("li");
    item.className = "account";
    const title = document.createElement("strong");
    title.textContent = entry.historyId;
    const details = document.createElement("span");
    details.textContent = `Attached locally · ${entry.createdAt}`;
    item.append(title, details);
    return item;
  }));
  if (entries.length === 0) {
    appendEmpty(lifecycleAttributions, "No local publication history is attached.");
  }
}

function renderLifecycleRelations(relations, selectedObjectId) {
  lifecycleRelations.replaceChildren(...relations.map((relation) => {
    const item = document.createElement("li");
    item.className = "account";
    const title = document.createElement("strong");
    const direction = relation.sourceBusinessObjectId === selectedObjectId ? "Outbound" : "Inbound";
    title.textContent = `${direction} · ${relation.relationType}`;
    const details = document.createElement("span");
    details.textContent = `${relation.sourceBusinessObjectId} → ${relation.targetBusinessObjectId} · ${relation.createdAt}`;
    item.append(title, details);
    return item;
  }));
  if (relations.length === 0) {
    appendEmpty(lifecycleRelations, "No inbound or outbound relations for this object.");
  }
}

async function refreshLifecycleDetails() {
  const object = selectedLifecycleObject();
  renderLifecycleRelationTargetOptions(object ? object.id : null);
  if (!object) {
    lifecycleLedger.replaceChildren();
    lifecycleAttributions.replaceChildren();
    lifecycleRelations.replaceChildren();
    appendEmpty(lifecycleLedger, "Select an object to view its ledger.");
    appendEmpty(lifecycleAttributions, "Select an object to view content attribution.");
    appendEmpty(lifecycleRelations, "Select an object to view related objects.");
    return;
  }
  lifecycleTransitionForm.elements.lifecycleStatus.value = object.lifecycleStatus;
  lifecycleTransitionForm.elements.approvalStatus.value = object.approvalStatus;
  try {
    const [ledgerEntries, attributionEntries, relations] = await Promise.all([
      invoke("lifecycle_ledger_entries", { input: { businessObjectId: object.id } }),
      invoke("lifecycle_content_attributions", { input: { businessObjectId: object.id } }),
      invoke("lifecycle_business_relations", { input: { businessObjectId: object.id } }),
    ]);
    renderLifecycleLedger(ledgerEntries);
    renderLifecycleAttributions(attributionEntries);
    renderLifecycleRelations(relations, object.id);
  } catch (error) {
    lifecycleLedger.replaceChildren();
    lifecycleAttributions.replaceChildren();
    lifecycleRelations.replaceChildren();
    lifecycleLedgerResult.textContent = `Unable to read lifecycle data: ${String(error)}`;
  }
}

async function refreshLifecycle() {
  try {
    renderLifecycleObjects(await invoke("lifecycle_objects"));
    await refreshLifecycleDetails();
  } catch (error) {
    lifecycleObjectResult.textContent = `Unable to read lifecycle objects: ${String(error)}`;
  }
}

document.querySelector("#refresh").addEventListener("click", refresh);
document.querySelector("#draft-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const selectedTargets = form.getAll("targets");
  try {
    const saved = await invoke("save_local_draft", {
      input: {
        title: form.get("title"),
        mediaPath: form.get("mediaPath"),
        targets: selectedTargets,
        scheduledAt: form.get("scheduledAt") || null,
      },
    });
    result.textContent = `Local draft ${saved.id} saved. No remote publish was attempted.`;
    event.currentTarget.reset();
    await refresh();
  } catch (error) {
    result.textContent = `Draft was not saved: ${String(error)}`;
  }
});

document.querySelector("#local-runner-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const providerRunners = String(form.get("providerRunners") || "")
    .split(/\r?\n/)
    .map((runner) => runner.trim())
    .filter(Boolean);
  if (form.get("confirmed") !== "on") {
    localRunnerResult.textContent = "Confirm the immediate local runner request before sending.";
    return;
  }
  try {
    const report = await invoke("dispatch_to_local_runner", {
      input: {
        title: form.get("title"),
        mediaPath: form.get("mediaPath"),
        targets: form.getAll("targets"),
        scheduledAt: null,
        providerRunners,
        confirmed: true,
      },
    });
    renderLocalRunnerOutcomes(report.outcomes);
    localRunnerResult.textContent = report.remotePublishConfirmed
      ? "Unexpected remote confirmation state."
      : "Local runner outcomes recorded. Remote platform publication is not confirmed.";
  } catch (error) {
    localRunnerOutcomes.replaceChildren();
    localRunnerResult.textContent = `Local runner request was not sent: ${String(error)}`;
  }
});

document.querySelector("#account-readiness-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  if (form.get("confirmed") !== "on") {
    accountReadinessResult.textContent = "Confirm the one-shot readiness check before sending.";
    return;
  }
  try {
    const report = await invoke("account_readiness", {
      input: {
        platform: form.get("platform"),
        providerRunner: form.get("providerRunner") || null,
        confirmed: true,
      },
    });
    accountReadinessResult.textContent = `Readiness: ${report.state}. This does not prove a completed login or remote publication.`;
  } catch (error) {
    accountReadinessResult.textContent = `Readiness check was not sent: ${String(error)}`;
  }
});

document.querySelector("#fanqie-review-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  if (form.get("confirmed") !== "on") {
    fanqieReviewResult.textContent = "Confirm the one-shot Fanqie review check before sending.";
    return;
  }
  try {
    const report = await invoke("fanqie_review_status", {
      input: {
        titleQuery: form.get("titleQuery"),
        providerRunner: form.get("providerRunner") || null,
        confirmed: true,
      },
    });
    fanqieReviewResult.textContent = `Fanqie status: ${report.state}. This does not prove remote publication.`;
  } catch (error) {
    fanqieReviewResult.textContent = `Fanqie review check was not sent: ${String(error)}`;
  }
});

document.querySelector("#account-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  try {
    const saved = await invoke("save_account", {
      input: {
        platform: form.get("platform"),
        displayName: form.get("displayName"),
        status: form.get("status"),
        phone: form.get("phone"),
        partition: form.get("partition"),
      },
    });
    accountResult.textContent = `Local account ${saved.id} saved. No credential or session data was stored.`;
    await refresh();
  } catch (error) {
    accountResult.textContent = `Account was not saved: ${String(error)}`;
  }
});

document.querySelector("#article-account-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  try {
    const saved = await invoke("save_article_account", {
      input: {
        displayName: form.get("displayName"),
        status: form.get("status"),
        phone: form.get("phone"),
        partition: form.get("partition"),
      },
    });
    articleAccountResult.textContent = `Local Juejin metadata ${saved.id} saved with status ${saved.status}. No browser or publish action was used.`;
    event.currentTarget.reset();
    await refresh();
  } catch (error) {
    articleAccountResult.textContent = `Juejin metadata was not saved: ${String(error)}`;
  }
});

historyForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  await refreshHistory();
});

document.querySelector("#refresh-lifecycle").addEventListener("click", refreshLifecycle);
lifecycleObjectSelect.addEventListener("change", refreshLifecycleDetails);

document.querySelector("#lifecycle-object-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  let attributes = {};
  const rawAttributes = String(form.get("attributes") || "").trim();
  if (rawAttributes) {
    try {
      attributes = JSON.parse(rawAttributes);
      if (!attributes || Array.isArray(attributes) || typeof attributes !== "object"
        || Object.values(attributes).some((value) => typeof value !== "string")) {
        throw new Error("attributes must be a JSON object with string values");
      }
    } catch (error) {
      lifecycleObjectResult.textContent = `Object was not created: ${String(error)}`;
      return;
    }
  }
  try {
    const object = await invoke("create_lifecycle_object", {
      input: {
        id: localId("object"),
        kind: form.get("kind"),
        displayName: form.get("displayName"),
        externalId: form.get("externalId") || null,
        attributes,
      },
    });
    lifecycleObjectResult.textContent = `Local object ${object.id} created.`;
    event.currentTarget.reset();
    await refreshLifecycle();
  } catch (error) {
    lifecycleObjectResult.textContent = `Object was not created: ${String(error)}`;
  }
});

document.querySelector("#lifecycle-ledger-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const object = selectedLifecycleObject();
  if (!object) {
    lifecycleLedgerResult.textContent = "Create and select an object before adding a ledger entry.";
    return;
  }
  const form = new FormData(event.currentTarget);
  try {
    const entry = await invoke("append_lifecycle_ledger_entry", {
      input: {
        id: localId("ledger"),
        businessObjectId: object.id,
        direction: form.get("direction"),
        category: form.get("category"),
        amountMinor: Number(form.get("amountMinor")),
        currency: String(form.get("currency")).toUpperCase(),
        approvalStatus: form.get("approvalStatus"),
        counterparty: form.get("counterparty") || null,
        reference: form.get("reference") || null,
        description: form.get("description") || null,
      },
    });
    lifecycleLedgerResult.textContent = `Immutable local ledger entry ${entry.id} appended.`;
    event.currentTarget.reset();
    await refreshLifecycleDetails();
  } catch (error) {
    lifecycleLedgerResult.textContent = `Ledger entry was not appended: ${String(error)}`;
  }
});

document.querySelector("#lifecycle-attribution-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const object = selectedLifecycleObject();
  if (!object) {
    lifecycleAttributionResult.textContent = "Create and select an object before attaching content.";
    return;
  }
  const form = new FormData(event.currentTarget);
  try {
    await invoke("add_lifecycle_content_attribution", {
      input: {
        businessObjectId: object.id,
        historyId: form.get("historyId"),
      },
    });
    lifecycleAttributionResult.textContent = "Existing local history was attached.";
    event.currentTarget.reset();
    await refreshLifecycleDetails();
  } catch (error) {
    lifecycleAttributionResult.textContent = `Content was not attached: ${String(error)}`;
  }
});

document.querySelector("#lifecycle-relation-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const object = selectedLifecycleObject();
  if (!object) {
    lifecycleRelationResult.textContent = "Create and select an object before adding a relation.";
    return;
  }
  if (!lifecycleRelationTargetSelect.value) {
    lifecycleRelationResult.textContent = "Create another object before adding a relation.";
    return;
  }
  const form = new FormData(event.currentTarget);
  let attributes = {};
  const rawAttributes = String(form.get("attributes") || "").trim();
  if (rawAttributes) {
    try {
      attributes = JSON.parse(rawAttributes);
      if (!attributes || Array.isArray(attributes) || typeof attributes !== "object"
        || Object.values(attributes).some((value) => typeof value !== "string")) {
        throw new Error("attributes must be a JSON object with string values");
      }
    } catch (error) {
      lifecycleRelationResult.textContent = `Relation was not added: ${String(error)}`;
      return;
    }
  }
  try {
    const relation = await invoke("add_lifecycle_business_relation", {
      input: {
        id: localId("relation"),
        sourceBusinessObjectId: object.id,
        targetBusinessObjectId: form.get("targetBusinessObjectId"),
        relationType: form.get("relationType"),
        attributes,
      },
    });
    lifecycleRelationResult.textContent = `Directed relation ${relation.id} added locally.`;
    event.currentTarget.reset();
    await refreshLifecycleDetails();
  } catch (error) {
    lifecycleRelationResult.textContent = `Relation was not added: ${String(error)}`;
  }
});

lifecycleTransitionForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const object = selectedLifecycleObject();
  if (!object) {
    lifecycleTransitionResult.textContent = "Create and select an object before changing its state.";
    return;
  }
  const form = new FormData(event.currentTarget);
  try {
    const transitioned = await invoke("transition_lifecycle_object", {
      input: {
        id: object.id,
        expectedRevision: object.revision,
        lifecycleStatus: form.get("lifecycleStatus"),
        approvalStatus: form.get("approvalStatus"),
      },
    });
    lifecycleTransitionResult.textContent = `Object transitioned to revision ${transitioned.revision}.`;
    await refreshLifecycle();
  } catch (error) {
    lifecycleTransitionResult.textContent = `Transition was not applied: ${String(error)}`;
  }
});

refresh();
