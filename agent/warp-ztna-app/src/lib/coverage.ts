const DOMAIN_SENTINEL = "192.0.2.0/32";

type Net = { family: 4 | 6; bits: bigint; prefix: number; mask: bigint };

export type Coverage = { nets: Net[]; domains: string[] };

function maskOf(prefix: number, length: number) {
  if (prefix <= 0) return 0n;
  return ((1n << BigInt(prefix)) - 1n) << BigInt(length - prefix);
}

function ipv4Bits(address: string) {
  const parts = address.split(".");
  if (parts.length !== 4) return null;
  let bits = 0n;
  for (const part of parts) {
    const octet = Number(part);
    if (!/^\d{1,3}$/.test(part) || octet > 255) return null;
    bits = (bits << 8n) | BigInt(octet);
  }
  return bits;
}

function ipv6Bits(address: string) {
  const halves = address.split("::");
  if (halves.length > 2) return null;
  const head = halves[0] ? halves[0].split(":") : [];
  const tail = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
  if (halves.length === 1 && head.length !== 8) return null;
  if (halves.length === 2 && head.length + tail.length >= 8) return null;
  const groups = [...head, ...Array(8 - head.length - tail.length).fill("0"), ...tail];
  if (groups.length !== 8) return null;
  let bits = 0n;
  for (const group of groups) {
    if (!/^[0-9a-f]{1,4}$/.test(group)) return null;
    bits = (bits << 16n) | BigInt(Number.parseInt(group, 16));
  }
  return bits;
}

function parseNet(value: string): Net | null {
  const [address, prefixText] = value.split("/");
  if (!address) return null;
  const family = address.includes(":") ? 6 : 4;
  const length = family === 4 ? 32 : 128;
  const bits = family === 4 ? ipv4Bits(address) : ipv6Bits(address);
  if (bits === null) return null;
  const prefix = prefixText === undefined ? length : Number(prefixText);
  if (!Number.isInteger(prefix) || prefix < 0 || prefix > length) return null;
  return { family, bits: bits & maskOf(prefix, length), prefix, mask: maskOf(prefix, length) };
}

function coversNet(route: Net, destination: Net) {
  if (route.family !== destination.family) return false;
  if (destination.prefix < route.prefix) return false;
  return (destination.bits & route.mask) === route.bits;
}

function coversDomain(destination: string, route: string) {
  const name = destination.startsWith("*.") ? destination.slice(2) : destination;
  if (!route.startsWith("*.")) return name === route;
  const base = route.slice(2);
  return name === base || name.endsWith(`.${base}`);
}

export function addressKind(address: string): "host" | "subnet" | "domain" {
  const net = parseNet(address.trim().toLowerCase());
  if (!net) return "domain";
  const hostPrefix = net.family === 4 ? 32 : 128;
  return net.prefix === hostPrefix ? "host" : "subnet";
}

export function normalizeRoutes(routes: string[]): Coverage {
  const nets: Net[] = [];
  const domains: string[] = [];
  for (const route of routes) {
    for (const part of route.split(",")) {
      const entry = part.trim().toLowerCase();
      if (!entry || entry === DOMAIN_SENTINEL) continue;
      const net = parseNet(entry);
      if (net) nets.push(net);
      else domains.push(entry);
    }
  }
  return { nets, domains };
}

export function isCovered(address: string, coverage: Coverage) {
  const target = address.trim().toLowerCase();
  if (!target) return false;
  const asNet = parseNet(target);
  if (asNet) return coverage.nets.some((route) => coversNet(route, asNet));
  return coverage.domains.some((route) => coversDomain(target, route));
}
