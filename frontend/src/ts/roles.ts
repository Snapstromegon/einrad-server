// Roles of the organizer team (ADR 0003): their names and the rules for
// granting them. Pure helpers, so the unit tests can import them without a
// DOM.
import type { schemas } from "./apiClient";

export type Role = schemas.Role;

/** Every role, in the order of the role table. */
export const ROLES: readonly Role[] = [
  "admin",
  "jury_lead",
  "treasurer",
  "check_in",
  "music_control",
  "moderation",
  "music_check",
];

export const roleNames: Record<Role, string> = {
  admin: "Admin",
  jury_lead: "Juryleitung",
  treasurer: "Kasse",
  check_in: "Check-in",
  music_control: "Musiksteuerung",
  moderation: "Moderation",
  music_check: "Musikprüfung",
};

/** `roles` with `role` granted (`on`) or revoked, in the table's order. */
export const withRole = (roles: readonly Role[], role: Role, on: boolean) =>
  ROLES.filter((r) => (r === role ? on : roles.includes(r)));

/**
 * Why `role` of `user` can't be revoked, or `null`: admins keep their own
 * Admin role, so there's always one left.
 */
export const roleLock = (
  user: { id: string; roles: readonly Role[] },
  role: Role,
  currentUserId: string | undefined,
) =>
  role === "admin" && user.roles.includes("admin") && user.id === currentUserId
    ? "Die eigene Admin-Rolle kann man nicht abgeben."
    : null;

export type Permission = schemas.Permission;

/**
 * What the organizer-side pages need: any one of the alternatives, each a
 * set of permissions that must all be held. Pages not listed are open.
 * Ordered like the admin navigation.
 */
const PAGES: Record<string, readonly (readonly Permission[])[]> = {
  "/admin": [["manage_users"]],
  "/admin-competition": [["manage_competition"]],
  "/admin-check-in": [["check_in", "view_riders"]],
  "/admin-riders": [["manage_registrations"]],
  "/admin-payments": [["manage_payments"]],
  "/admin-acts-overview": [["plan_competition"], ["check_songs"]],
  // The jury lead places the finals there.
  "/admin-categories": [["plan_competition"], ["manage_results"]],
  "/admin-timeplan": [["plan_competition"]],
  "/admin-music-control": [["control_music"]],
  "/admin-awards": [["control_music"]],
  "/admin-medals": [["view_medals"]],
  "/admin-judges": [["manage_jury"]],
  "/admin-mails": [["mail_clubs"]],
  "/admin-moderation": [["view_moderation"]],
  "/player": [["control_music"]],
};

/** Whether `permissions` open the page at `path`. */
export const canOpen = (path: string, permissions: readonly Permission[]) =>
  PAGES[path]?.some((needed) => needed.every((p) => permissions.includes(p))) ??
  true;

/** The organizer-side pages `permissions` open, in navigation order. */
export const adminPages = (permissions: readonly Permission[]) =>
  Object.keys(PAGES).filter((path) => canOpen(path, permissions));

/** Whether `user` holds `permission`. */
export const can = (
  user: { permissions: readonly Permission[] } | null | undefined,
  permission: Permission,
) => !!user?.permissions.includes(permission);
