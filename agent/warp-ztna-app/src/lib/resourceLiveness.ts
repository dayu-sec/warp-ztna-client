import type { ResourceRow } from "../api/access";

export type LivenessState = Map<string, boolean>;

export type LivenessFlip = { address: string; live: boolean };

export type LivenessTr = (key: string, vars?: Record<string, string>) => string;

export function livenessState(rows: ResourceRow[]): LivenessState {
  return new Map(rows.map((row) => [row.key, row.live]));
}

export function livenessFlips(previous: LivenessState | null, rows: ResourceRow[]): LivenessFlip[] {
  if (!previous) return [];
  return rows
    .filter((row) =>
      previous.has(row.key)
        ? previous.get(row.key) !== row.live
        : row.live && row.source === "ticket",
    )
    .map((row) => ({ address: row.address, live: row.live }));
}

export function describeFlips(flips: LivenessFlip[], tr: LivenessTr): string | null {
  if (flips.length === 0) return null;
  const up = flips.filter((flip) => flip.live);
  const down = flips.filter((flip) => !flip.live);
  if (up.length > 0 && down.length > 0) {
    return tr("resources.livenessMixed", { up: String(up.length), down: String(down.length) });
  }
  const group = up.length > 0 ? up : down;
  if (group.length === 1) {
    return tr(up.length > 0 ? "resources.livenessUp" : "resources.livenessDown", {
      name: group[0].address,
    });
  }
  return tr(up.length > 0 ? "resources.livenessUpMany" : "resources.livenessDownMany", {
    count: String(group.length),
  });
}
