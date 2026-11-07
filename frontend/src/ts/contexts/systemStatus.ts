import { createContext } from "@lit/context";
import type { schemas } from "../apiClient";

export type SystemStatus = schemas.SystemStatus;
/** Fired on `window` after the competition changed: the status reloads. */
export const SYSTEM_STATUS_CHANGED = "cup-system-status-changed";

export const systemStatusContext = createContext<SystemStatus | null>(
  Symbol("system_status"),
);
