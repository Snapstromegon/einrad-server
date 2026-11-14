import { css, html, LitElement } from "lit";
import { customElement } from "lit/decorators.js";
import { adminStyles } from "../../styles/admin";
import { resetStyles } from "../../styles/base";
import "../elements/cup-db-backups.js";

/**
 * Database backups (Admin, `manage_backups`): list, create, download, delete
 * and restore backups, and the songs a restore leaves missing.
 */
@customElement("cup-view-admin-backups")
export default class CupViewAdminBackups extends LitElement {
  static override styles = [
    resetStyles,
    adminStyles,
    css`
      cup-db-backups {
        max-width: 72rem;
      }
    `,
  ];

  override render() {
    return html`<div class="page-pad">
      <h1>Backups</h1>
      <cup-db-backups></cup-db-backups>
    </div>`;
  }
}
