import { provide } from "@lit/context";
import { html, LitElement } from "lit";
import { customElement } from "lit/decorators.js";
import { unwrap } from "../../api";
import { client } from "../../apiClient.js";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus.js";
import "./cup-centered-icon-box.js";
import "../views/cup-view-setup.js";
import { Task } from "@lit/task";

/**
 * The pages that work without a competition (setup mode): the import and
 * what an admin may need to log in.
 */
const SETUP_PAGES = [
  "/admin-competition",
  "/request_password_reset",
  "/reset_password",
];

/**
 * Provides the system status. Without a competition (setup mode) only the
 * `SETUP_PAGES` show; every other page shows "noch nicht eröffnet".
 */
@customElement("cup-context-system-status")
export default class CupContextSystemStatus extends LitElement {
  @provide({ context: systemStatusContext }) systemStatus: SystemStatus | null =
    null;

  SystemStatusTask = new Task(this, {
    task: async () => {
      this.systemStatus = await unwrap(client.getSystemStatus());
      return this.systemStatus;
    },
    args: () => [],
  });

  override render() {
    return this.SystemStatusTask.render({
      complete: (status) =>
        status.competition || SETUP_PAGES.includes(location.pathname)
          ? html`<slot></slot>`
          : html`<cup-view-setup></cup-view-setup>`,
      error: (error) => html`<p>${error}</p>`,
    });
  }
}
