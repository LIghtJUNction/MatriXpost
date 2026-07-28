const invoke = window.__TAURI__.core.invoke;

const platforms = document.querySelector("#platforms");
const targets = document.querySelector("#target-options");
const summary = document.querySelector("#summary");
const accounts = document.querySelector("#accounts");
const result = document.querySelector("#draft-result");
const accountResult = document.querySelector("#account-result");
const articleAccounts = document.querySelector("#article-accounts");
const articleAccountResult = document.querySelector("#article-account-result");
const historyForm = document.querySelector("#history-form");
const history = document.querySelector("#history");
const historyResult = document.querySelector("#history-result");

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
  } catch (error) {
    result.textContent = `Unable to read local state: ${String(error)}`;
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

refresh();
