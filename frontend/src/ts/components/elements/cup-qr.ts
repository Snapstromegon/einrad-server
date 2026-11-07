import { css, html, LitElement, nothing, svg } from "lit";
import { customElement, property } from "lit/decorators.js";
import { encode } from "uqr";

/**
 * Renders `value` as a QR code: black on white with a quiet zone, also in
 * dark mode and in print, so scanners can read it. Renders nothing if the
 * value is empty or doesn't fit into `max-version`.
 *
 * `<cup-qr value="https://…" label="QR-Code zu den Fotos"></cup-qr>`
 */
@customElement("cup-qr")
export default class CupQr extends LitElement {
  static override styles = css`
    :host {
      display: inline-block;
      width: 180px;
      max-width: 100%;
    }

    svg {
      display: block;
      width: 100%;
      height: auto;
      /* Scanners need dark modules on a light background, in dark mode too. */
      background: white; /* color-literal-ok: QR code */
      print-color-adjust: exact;
    }

    path {
      fill: black; /* color-literal-ok: QR code */
    }
  `;

  @property() value = "";
  /** Read out instead of the code. */
  @property() label = "QR-Code";
  /** The largest QR version allowed, e.g. 13 for EPC payment codes. */
  @property({ type: Number, attribute: "max-version" }) maxVersion = 40;

  override render() {
    if (!this.value) {
      return nothing;
    }
    let data: boolean[][];
    try {
      // As UTF-8 bytes, without an ECI header that some scanners don't expect.
      const bytes = Array.from(new TextEncoder().encode(this.value));
      ({ data } = encode(bytes, {
        ecc: "M",
        maxVersion: this.maxVersion,
        border: 4,
      }));
    } catch (error) {
      console.error("QR code failed", error);
      return nothing;
    }
    const path = data
      .flatMap((row, y) =>
        row.map((dark, x) => (dark ? `M${x} ${y}h1v1h-1z` : "")),
      )
      .join("");
    return html`<svg
      viewBox="0 0 ${data.length} ${data.length}"
      role="img"
      aria-label=${this.label}
      shape-rendering="crispEdges"
    >
      ${svg`<path d=${path} />`}
    </svg>`;
  }
}
