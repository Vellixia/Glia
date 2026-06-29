import { describe, it, expect } from "vitest";
import {
  skillKeys,
  agentKeys,
  settingsKeys,
  syncKeys,
  catalogKeys,
  secretsKeys,
} from "./query-keys";

describe("skillKeys", () => {
  it("all returns ['skills']", () => {
    expect(skillKeys.all).toEqual(["skills"]);
  });

  it("lists returns ['skills', 'list']", () => {
    expect(skillKeys.lists()).toEqual(["skills", "list"]);
  });

  it("list returns ['skills', 'list', { filters, first }]", () => {
    const result = skillKeys.list({ category: "communication" }, 10);
    expect(result).toEqual(["skills", "list", { filters: { category: "communication" }, first: 10 }]);
  });

  it("details returns ['skills', 'detail']", () => {
    expect(skillKeys.details()).toEqual(["skills", "detail"]);
  });

  it("detail returns ['skills', 'detail', id]", () => {
    expect(skillKeys.detail("abc-123")).toEqual(["skills", "detail", "abc-123"]);
  });
});

describe("agentKeys", () => {
  it("all returns ['agents']", () => {
    expect(agentKeys.all).toEqual(["agents"]);
  });

  it("lists returns ['agents', 'list']", () => {
    expect(agentKeys.lists()).toEqual(["agents", "list"]);
  });

  it("list returns ['agents', 'list', filters] when filters provided", () => {
    const result = agentKeys.list({ role: "worker" });
    expect(result).toEqual(["agents", "list", { role: "worker" }]);
  });

  it("list returns ['agents', 'list', undefined] when no filters", () => {
    const result = agentKeys.list();
    expect(result).toEqual(["agents", "list", undefined]);
  });
});

describe("settingsKeys", () => {
  it("all returns ['settings']", () => {
    expect(settingsKeys.all).toEqual(["settings"]);
  });
});

describe("syncKeys", () => {
  it("all returns ['sync']", () => {
    expect(syncKeys.all).toEqual(["sync"]);
  });

  it("status returns ['sync', 'status']", () => {
    expect(syncKeys.status()).toEqual(["sync", "status"]);
  });
});

describe("catalogKeys", () => {
  it("all returns ['catalog']", () => {
    expect(catalogKeys.all).toEqual(["catalog"]);
  });

  it("tools returns ['catalog', 'tools']", () => {
    expect(catalogKeys.tools()).toEqual(["catalog", "tools"]);
  });

  it("installed returns ['catalog', 'installed']", () => {
    expect(catalogKeys.installed()).toEqual(["catalog", "installed"]);
  });
});

describe("secretsKeys", () => {
  it("all returns ['secrets']", () => {
    expect(secretsKeys.all).toEqual(["secrets"]);
  });

  it("providers returns ['secrets', 'providers']", () => {
    expect(secretsKeys.providers()).toEqual(["secrets", "providers"]);
  });

  it("credentials returns ['secrets', 'credentials']", () => {
    expect(secretsKeys.credentials()).toEqual(["secrets", "credentials"]);
  });
});
