// The club contact in the admin tables: name, email and whether it's
// verified; "–" for whoever may not see club contacts.
import { html, nothing } from "lit";
import type { schemas } from "../../apiClient";

export const renderContact = (
  contacts: schemas.ClubContacts | null | undefined,
) =>
  contacts
    ? html`${contacts.contact_name}
        <a
          class="email"
          href="mailto:${contacts.contact_email}"
          title=${contacts.contact_email}
          >${contacts.contact_email}</a
        >
        ${
          contacts.contact_email_verified
            ? nothing
            : html`<span class="chip warn">unverifiziert</span>`
        }`
    : "–";
