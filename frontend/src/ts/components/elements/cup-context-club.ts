import { provide } from "@lit/context";
import { html, LitElement } from "lit";
import { customElement, property } from "lit/decorators.js";
import { unwrap } from "../../api";
import { client } from "../../apiClient.js";
import { type Club, clubContext } from "../../contexts/club.js";
import "./cup-centered-icon-box.js";
import { Task } from "@lit/task";

@customElement("cup-context-club")
export default class CupContextClub extends LitElement {
  @provide({ context: clubContext }) club: Club | null = null;
  @property({ attribute: "club-id" }) clubId?: string;

  clubTask = new Task(this, {
    task: async ([clubId]) => {
      if (!clubId) {
        throw new Error("Vereins ID fehlt!");
      }
      this.club = await unwrap(client.getClub({ query: { club_id: clubId } }));
      return this.club;
    },
    args: () => [this.clubId],
  });

  override render() {
    return this.clubTask.render({
      complete: () => html`<slot></slot>`,
      error: (error) => html`<p>${error}</p>`,
      pending: () => html`<p>Lädt Verein...</p>`,
    });
  }
}
