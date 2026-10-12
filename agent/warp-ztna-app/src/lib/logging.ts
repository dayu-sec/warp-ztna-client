import { debug, error, info, trace, warn } from "@tauri-apps/plugin-log";
import { isTauri } from "../api/native";

type ConsoleName = "log" | "debug" | "info" | "warn" | "error";
type ConsoleFn = (...args: unknown[]) => void;
type PluginLogger = (message: string) => Promise<void>;

const FORWARDING_SET_KEY = "__warp_ztna_console_forwarding__";

const loggers: Record<ConsoleName, PluginLogger> = {
  log: trace,
  debug,
  info,
  warn,
  error,
};

function formatArg(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) return value.stack ?? value.message;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function formatArgs(args: unknown[]): string {
  return args.map(formatArg).join(" ");
}

export async function setupLogging(): Promise<void> {
  if (!isTauri()) return;

  const globalRecord = globalThis as typeof globalThis & {
    [FORWARDING_SET_KEY]?: boolean;
  };
  if (globalRecord[FORWARDING_SET_KEY]) return;
  globalRecord[FORWARDING_SET_KEY] = true;

  const consoleRef = console as Console & Record<ConsoleName, ConsoleFn>;
  const original: Record<ConsoleName, ConsoleFn> = {
    log: consoleRef.log.bind(console),
    debug: consoleRef.debug.bind(console),
    info: consoleRef.info.bind(console),
    warn: consoleRef.warn.bind(console),
    error: consoleRef.error.bind(console),
  };

  for (const name of Object.keys(loggers) as ConsoleName[]) {
    consoleRef[name] = (...args: unknown[]) => {
      original[name](...args);
      void loggers[name](formatArgs(args)).catch(() => undefined);
    };
  }
}
