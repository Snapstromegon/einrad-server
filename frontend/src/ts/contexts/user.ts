import { createContext } from "@lit/context";
import type { schemas } from "../apiClient";

/** The logged-in user with what their roles allow. */
export type User = schemas.CurrentUser;
export const userContext = createContext<User | null>(Symbol("user"));
