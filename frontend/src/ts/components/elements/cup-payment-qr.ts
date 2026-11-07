import { css, html, LitElement } from "lit";
import { customElement, property } from "lit/decorators.js";
import "./cup-qr.js";

/**
 * Renders an EPC payload (see `epc.ts`) as a QR code. EPC codes may use at
 * most QR version 13; renders nothing if the payload doesn't fit.
 */
@customElement("cup-payment-qr")
export default class CupPaymentQr extends LitElement {
  static override styles = css`
    :host {
      display: inline-block;
      width: 180px;
      max-width: 100%;
    }

    cup-qr {
      display: block;
      width: 100%;
    }
  `;

  @property() payload = "";

  override render() {
    return html`<cup-qr
      .value=${this.payload}
      label="QR-Code für die Überweisung"
      max-version="13"
    ></cup-qr>`;
  }
}
