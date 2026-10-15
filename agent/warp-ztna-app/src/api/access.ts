import { addressKind, isCovered, normalizeRoutes, type Coverage } from "../lib/coverage.ts";

export type MeshAddressKind = "host" | "subnet" | "domain";

type PortRange = { start: number; end: number };

export type AccessRequestTarget = {
  ordinal: number;
  target_kind: "asset" | "custom";
  asset_id?: string | null;
  address?: string | null;
  name_snapshot?: string | null;
  addresses_snapshot?: string[];
  protocol: string;
  ports?: number[];
  port_ranges?: PortRange[];
};

export type SshCaGrantRef = {
  status?: string | null;
  error?: string | null;
};

export type AccessRequestGrantRefs = {
  ssh_ca?: SshCaGrantRef | null;
} | null;

export type AccessRequest = {
  id: string;
  target_mode: "asset" | "custom";
  targets: AccessRequestTarget[];
  status: string;
  expires_at?: string | null;
  ssh_scope?: boolean;
  grant_refs?: AccessRequestGrantRefs;
};

type CatalogNodeAddress = { address: string; kind: MeshAddressKind };

type CatalogAddress = {
  address: string;
  kind: MeshAddressKind;
  name: string;
  managed: boolean;
  assets: string[];
};

export type AccessRequestCatalog = {
  groups: {
    id: string;
    name: string;
    parent_id?: string | null;
    nodes: { id: string; name: string; addresses: CatalogNodeAddress[] }[];
  }[];
  addresses: CatalogAddress[];
};

export type SshIssueState = "issued" | "pending_host" | "failed" | "pending";

export type RowSsh = { state: SshIssueState; requestId: string; error?: string | null };

export type GroupGrant = {
  address: string;
  protocol: string;
  ports?: number[];
  port_ranges?: PortRange[];
  group_name?: string;
};

export type RowSource = "ticket" | "group";

export type ResourceRow = {
  key: string;
  address: string;
  kind: MeshAddressKind;
  protocol: string;
  ports: string;
  effective: boolean;
  live: boolean;
  pending: boolean;
  ssh: RowSsh | null;
  source: RowSource;
};

type ResourceAsset = { id: string; name: string; rows: ResourceRow[] };

export type ResourceSystem = { id: string; name: string; assets: ResourceAsset[]; rows: ResourceRow[] };

export const UNGROUPED_SYSTEM = "__ungrouped__";

export function addressKey(address: string) {
  const value = address.trim().toLowerCase();
  if (value.endsWith("/32")) return value.slice(0, -3);
  if (value.endsWith("/128")) return value.slice(0, -4);
  return value;
}

type PortSpec = { protocol: string; ports?: number[]; port_ranges?: PortRange[] };

export function portsText(spec: PortSpec) {
  const ports = (spec.ports ?? []).map(String);
  const ranges = (spec.port_ranges ?? []).map((range) => `${range.start}-${range.end}`);
  return [...ports, ...ranges].join(", ");
}

function buildIndexes(catalog: AccessRequestCatalog) {
  const groupById = new Map(catalog.groups.map((group) => [group.id, group]));
  const rootName = (group: AccessRequestCatalog["groups"][number]): string => {
    const seen = new Set<string>([group.id]);
    let current = group;
    while (current.parent_id) {
      const parent = groupById.get(current.parent_id);
      if (!parent || seen.has(parent.id)) break;
      seen.add(parent.id);
      current = parent;
    }
    return current.name;
  };
  const systemByAsset = new Map<string, string>();
  const systemByAddress = new Map<string, string>();
  const assetByAddress = new Map<string, string>();
  const nameByAsset = new Map<string, string>();
  const kindByAddress = new Map<string, MeshAddressKind>();
  for (const group of catalog.groups) {
    const system = rootName(group);
    systemByAsset.set(group.id, system);
    for (const node of group.nodes) {
      systemByAsset.set(node.id, system);
      nameByAsset.set(node.id, node.name);
      for (const address of node.addresses) {
        if (!kindByAddress.has(addressKey(address.address))) {
          kindByAddress.set(addressKey(address.address), address.kind);
        }
      }
    }
  }
  for (const entry of catalog.addresses) {
    if (!kindByAddress.has(addressKey(entry.address))) {
      kindByAddress.set(addressKey(entry.address), entry.kind);
    }
    const key = addressKey(entry.address);
    if (systemByAddress.has(key)) continue;
    for (const asset of entry.assets) {
      const system = systemByAsset.get(asset);
      if (system) {
        systemByAddress.set(key, system);
        assetByAddress.set(key, asset);
        break;
      }
    }
  }
  return { systemByAsset, systemByAddress, assetByAddress, nameByAsset, kindByAddress };
}

function addressesOf(target: AccessRequestTarget) {
  if (target.address?.trim()) return [target.address.trim()];
  return (target.addresses_snapshot ?? []).map((value) => value.trim()).filter(Boolean);
}

function targetAllowsSsh(target: AccessRequestTarget) {
  const protocol = target.protocol.toLowerCase();
  if (protocol !== "tcp" && protocol !== "all") return false;
  if ((target.ports ?? []).includes(22)) return true;
  return (target.port_ranges ?? []).some((range) => range.start <= 22 && 22 <= range.end);
}

export function isSshAddress(address: string, kind: MeshAddressKind) {
  if (kind === "subnet") return false;
  const normalised = addressKey(address);
  return !normalised.includes("/") && !normalised.includes("*");
}

const SSH_STATE_RANK: Record<SshIssueState, number> = {
  issued: 3,
  pending_host: 2,
  failed: 1,
  pending: 0,
};

function rowSsh(
  request: AccessRequest,
  target: AccessRequestTarget,
  address: string,
  kind: MeshAddressKind,
): RowSsh | null {
  if (!targetAllowsSsh(target) || !isSshAddress(address, kind)) return null;
  const sshCa = request.grant_refs?.ssh_ca;
  const status = sshCa?.status;
  const state: SshIssueState =
    status === "issued" || status === "pending_host" || status === "failed" ? status : "pending";
  return { state, requestId: request.id, error: sshCa?.error };
}

function mergeRowSsh(row: ResourceRow, candidate: RowSsh | null) {
  if (!candidate) return;
  if (!row.ssh || SSH_STATE_RANK[candidate.state] > SSH_STATE_RANK[row.ssh.state]) {
    row.ssh = candidate;
  }
}

function isEffective(request: AccessRequest) {
  if (request.status !== "authorized") return false;
  if (!request.expires_at) return true;
  const expiresAt = Date.parse(request.expires_at);
  return Number.isFinite(expiresAt) && expiresAt > Date.now();
}

export function mapAccessResources(
  requests: AccessRequest[],
  catalog: AccessRequestCatalog,
  groupGrants: GroupGrant[],
  routes: string[],
  connected: boolean,
): ResourceSystem[] {
  const coverage: Coverage = normalizeRoutes(routes);
  const { systemByAsset, systemByAddress, assetByAddress, nameByAsset, kindByAddress } =
    buildIndexes(catalog);
  const systems = new Map<string, ResourceSystem>();
  const seen = new Map<string, ResourceRow>();

  const placeRow = (row: ResourceRow, systemName: string, asset: { id: string; name?: string } | null) => {
    const system = systems.get(systemName) ?? { id: systemName, name: systemName, assets: [], rows: [] };
    systems.set(systemName, system);
    if (!asset) {
      system.rows.push(row);
      return;
    }
    const nodeId = `${systemName}:${asset.id}`;
    const node =
      system.assets.find((entry) => entry.id === nodeId) ??
      (() => {
        const created = { id: nodeId, name: asset.name ?? asset.id, rows: [] };
        system.assets.push(created);
        return created;
      })();
    node.rows.push(row);
  };

  const takeRow = (
    spec: PortSpec,
    address: string,
    effective: boolean,
    source: RowSource,
  ): ResourceRow => {
    const key = addressKey(address);
    const ports = portsText(spec);
    return {
      key: `${key}|${spec.protocol.toLowerCase()}|${ports}`,
      address,
      kind: kindByAddress.get(key) ?? addressKind(address),
      protocol: spec.protocol.toUpperCase(),
      ports,
      effective,
      live: effective && connected && isCovered(address, coverage),
      pending: !effective || (connected && !isCovered(address, coverage)),
      ssh: null,
      source,
    };
  };

  for (const request of requests) {
    const effective = isEffective(request);
    for (const target of request.targets ?? []) {
      const addresses = addressesOf(target);
      for (const address of addresses) {
        const row = takeRow(target, address, effective, "ticket");
        const existing = seen.get(row.key);
        if (existing) {
          mergeRowSsh(existing, rowSsh(request, target, address, existing.kind));
          continue;
        }
        seen.set(row.key, row);
        mergeRowSsh(row, rowSsh(request, target, address, row.kind));
        const asset =
          target.target_kind === "custom" || !target.asset_id ? null : { id: target.asset_id };
        const systemName =
          (asset ? systemByAsset.get(asset.id) : undefined) ??
          systemByAddress.get(addressKey(address)) ??
          UNGROUPED_SYSTEM;
        placeRow(
          row,
          systemName,
          asset
            ? { id: asset.id, name: nameByAsset.get(asset.id) ?? target.name_snapshot ?? asset.id }
            : null,
        );
      }
    }
  }

  for (const grant of groupGrants ?? []) {
    const address = grant.address.trim();
    const key = addressKey(address);
    const row = takeRow(grant, address, true, "group");
    if (seen.has(row.key)) continue;
    seen.set(row.key, row);
    const owner = assetByAddress.get(key);
    if (owner) {
      placeRow(row, systemByAsset.get(owner) ?? UNGROUPED_SYSTEM, { id: owner, name: nameByAsset.get(owner) });
      continue;
    }
    placeRow(row, systemByAddress.get(key) ?? grant.group_name ?? UNGROUPED_SYSTEM, null);
  }

  return [...systems.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export function collectRows(systems: ResourceSystem[]) {
  const rows: ResourceRow[] = [];
  for (const system of systems) {
    rows.push(...system.rows);
    for (const asset of system.assets) rows.push(...asset.rows);
  }
  return rows.sort((a, b) => a.address.localeCompare(b.address));
}

export function isLaunchable(address: string, kind: MeshAddressKind) {
  if (kind !== "domain") return false;
  return !address.includes("/") && !address.includes("*");
}
