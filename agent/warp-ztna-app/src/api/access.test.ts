import assert from "node:assert/strict";
import { test } from "node:test";

import { collectRows, mapAccessResources, UNGROUPED_SYSTEM } from "./access.ts";
import type { AccessRequest, AccessRequestCatalog, GroupGrant } from "./access.ts";

const catalog: AccessRequestCatalog = {
  groups: [
    {
      id: "asset-1",
      name: "示例业务系统",
      nodes: [{ id: "node-1", name: "示例应用", addresses: [{ address: "192.0.2.53", kind: "host" }] }],
    },
  ],
  addresses: [
    { address: "192.0.2.53", kind: "host", name: "示例应用", managed: true, assets: ["node-1"] },
  ],
};

const ticket: AccessRequest = {
  id: "request-1",
  target_mode: "asset",
  status: "authorized",
  expires_at: null,
  targets: [
    {
      ordinal: 0,
      target_kind: "asset",
      asset_id: "node-1",
      addresses_snapshot: ["192.0.2.53"],
      protocol: "tcp",
      ports: [443],
    },
  ],
};

function grant(address: string, protocol: string, ports: number[], groupName?: string): GroupGrant {
  return { address, protocol, ports, group_name: groupName };
}

test("group grants join the tree under the asset the catalog knows", () => {
  const systems = mapAccessResources(
    [],
    catalog,
    [grant("192.0.2.53", "tcp", [443], "示例数据库")],
    [],
    true,
  );

  assert.equal(systems.length, 1);
  assert.equal(systems[0].name, "示例业务系统");
  assert.equal(systems[0].assets.length, 1);
  assert.equal(systems[0].assets[0].name, "示例应用");
  assert.equal(systems[0].assets[0].rows[0].source, "group");
  assert.equal(systems[0].assets[0].rows[0].protocol, "TCP");
  assert.equal(systems[0].assets[0].rows[0].ssh, null);
});

const nestedCatalog: AccessRequestCatalog = {
  groups: [
    {
      id: "system-1",
      name: "业务系统",
      nodes: [],
    },
    {
      id: "sub-1",
      name: "子系统",
      parent_id: "system-1",
      nodes: [{ id: "node-2", name: "子系统应用", addresses: [{ address: "192.0.2.80", kind: "host" }] }],
    },
  ],
  addresses: [
    { address: "192.0.2.80", kind: "host", name: "子系统应用", managed: true, assets: ["node-2"] },
  ],
};

test("nested groups resolve to the top-level business system", () => {
  const systems = mapAccessResources([], nestedCatalog, [grant("192.0.2.80", "tcp", [443])], ["192.0.2.0/24"], true);

  assert.equal(systems.length, 1);
  assert.equal(systems[0].name, "业务系统");
  assert.equal(systems[0].assets[0].name, "子系统应用");
});

test("group grants the catalog cannot place fall back to the resource group name", () => {
  const systems = mapAccessResources([], catalog, [grant("192.0.2.9", "tcp", [5432], "示例数据库")], [], true);

  assert.equal(systems[0].name, "示例数据库");
  assert.equal(systems[0].rows[0].address, "192.0.2.9");
});

test("group grants without a resource group name land in the ungrouped bucket", () => {
  const systems = mapAccessResources([], catalog, [grant("192.0.2.9", "tcp", [5432])], [], true);

  assert.equal(systems[0].name, UNGROUPED_SYSTEM);
});

test("a ticket wins the row when both sources grant the same address and ports", () => {
  const systems = mapAccessResources(
    [ticket],
    catalog,
    [grant("192.0.2.53", "tcp", [443], "示例数据库")],
    [],
    true,
  );
  const rows = collectRows(systems);

  assert.equal(rows.length, 1);
  assert.equal(rows[0].source, "ticket");
});

test("the same address on different ports keeps one row per grant", () => {
  const systems = mapAccessResources(
    [ticket],
    catalog,
    [grant("192.0.2.53", "tcp", [5432], "示例数据库")],
    [],
    true,
  );
  const rows = collectRows(systems);

  assert.equal(rows.length, 2);
  assert.deepEqual(
    rows.map((row) => [row.ports, row.source]).sort(),
    [
      ["443", "ticket"],
      ["5432", "group"],
    ],
  );
});

test("group grants are marked active only when the daemon route covers them", () => {
  const covered = mapAccessResources([], catalog, [grant("192.0.2.53", "tcp", [443])], ["192.0.2.0/24"], true);
  const uncovered = mapAccessResources([], catalog, [grant("192.0.2.53", "tcp", [443])], [], true);

  assert.equal(collectRows(covered)[0].live, true);
  assert.equal(collectRows(covered)[0].pending, false);
  assert.equal(collectRows(uncovered)[0].live, false);
  assert.equal(collectRows(uncovered)[0].pending, true);
});

test("effectiveness holds across a disconnect so the tray status cannot jump", () => {
  const online = mapAccessResources([], catalog, [grant("192.0.2.53", "tcp", [443])], ["192.0.2.0/24"], true);
  const offline = mapAccessResources([], catalog, [grant("192.0.2.53", "tcp", [443])], [], false);

  assert.equal(collectRows(online)[0].effective, true);
  assert.equal(collectRows(offline)[0].effective, true);
  assert.equal(collectRows(offline)[0].live, false);
});

test("an expired ticket is no longer effective", () => {
  const expired = { ...ticket, expires_at: "2020-01-01T00:00:00Z" };
  const systems = mapAccessResources([expired], catalog, [], [], true);

  assert.equal(collectRows(systems)[0].effective, false);
});
