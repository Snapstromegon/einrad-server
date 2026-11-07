import { createContext } from "@lit/context";
import type { schemas } from "../apiClient";

export type Club = schemas.Club;
export const clubContext = createContext<Club | null>(Symbol("club"));
