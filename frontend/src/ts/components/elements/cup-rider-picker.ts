import { css, html, LitElement, nothing } from "lit";
import { customElement, property, query, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { normalizeName } from "../../pairs";

/** A choice of the picker; `group` puts it under a heading (e.g. a club). */
export interface PickerOption {
  value: string;
  label: string;
  /** Shown after the label, e.g. "(RV Bonn)" or "– nennt Laura Schulz". */
  detail?: string;
  group?: string;
}

let nextId = 0;

/**
 * A searchable dropdown, e.g. for riders: typing narrows the list down
 * (case and accents don't matter), arrow keys and Enter pick, Escape
 * cancels. Options with a `group` are listed under its heading, in the
 * order given. The list opens in the top layer (popover), so scrolling
 * containers don't clip it; while open it follows the field every frame,
 * whatever scrolls, and closes once the field scrolls out of sight.
 * Dispatches `change` with `detail.value`; it doesn't change `value`
 * itself, the owner does after saving.
 */
@customElement("cup-rider-picker")
export default class CupRiderPicker extends LitElement {
  static override styles = css`
    :host {
      display: inline-block;
      width: 16rem;
      max-width: 100%;
      vertical-align: middle;
    }

    * {
      box-sizing: border-box;
    }

    .field {
      position: relative;
      display: flex;
      align-items: center;
    }

    input {
      width: 100%;
      font: inherit;
      font-size: var(--picker-font-size, 1rem);
      color: var(--color-text);
      padding: var(--picker-padding, 0.4rem 1.75rem 0.4rem 0.5rem);
      border: 1px solid var(--color-border);
      border-radius: 0.375rem;
      background: var(--color-surface);
      text-overflow: ellipsis;
    }

    input::placeholder {
      color: var(--picker-placeholder, var(--color-text-muted));
    }

    input:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 0;
      border-color: var(--color-primary-strong);
    }

    input:disabled {
      background: var(--color-surface-2);
      color: var(--color-text-muted);
    }

    .chevron {
      position: absolute;
      right: 0.5rem;
      pointer-events: none;
      color: var(--color-text-muted);
      font-family: "Material Symbols Outlined";
      font-size: 18px;
      line-height: 1;
    }

    [popover] {
      position: fixed;
      inset: auto;
      margin: 0;
      padding: 0.25rem 0;
      max-height: min(20rem, 50vh);
      overflow-y: auto;
      border: 1px solid var(--color-border);
      border-radius: 0.5rem;
      background: var(--color-surface);
      color: var(--color-text);
      box-shadow: 0 6px 24px var(--color-shadow);
      font-size: 0.9rem;
    }

    ul {
      list-style: none;
      margin: 0;
      padding: 0;
    }

    .heading {
      padding: 0.375rem 0.75rem 0.125rem;
      font-size: 0.75rem;
      font-weight: 700;
      color: var(--color-text-muted);
      text-transform: uppercase;
      letter-spacing: 0.03em;
    }

    .option {
      padding: 0.3rem 0.75rem;
      cursor: pointer;
    }

    .option.active {
      background: var(--color-primary-soft);
    }

    .option.selected {
      font-weight: 700;
    }

    .option .detail {
      color: var(--color-text-muted);
    }

    .empty {
      padding: 0.375rem 0.75rem;
      color: var(--color-text-muted);
    }
  `;

  @property({ attribute: false }) options: PickerOption[] = [];
  /** The selected value, `""` for none. */
  @property() value = "";
  @property() placeholder = "FahrerIn suchen…";
  /** The accessible name. */
  @property() label = "";
  @property({ type: Boolean }) disabled = false;

  @state() open = false;
  @state() query = "";
  @state() active = 0;

  @query("input") input!: HTMLInputElement;
  @query("[popover]") list!: HTMLElement;

  // eslint-disable-next-line no-useless-assignment -- read by the next picker
  private readonly listId = `rider-picker-${nextId++}`;
  /** The animation frame that keeps the list at the field. */
  private frame = 0;
  /** Where the list was put last, to only touch the styles on changes. */
  private placed = "";

  get selected() {
    return this.options.find((o) => o.value === this.value);
  }

  /** The options matching the typed words. */
  get filtered() {
    const words = normalizeName(this.query).split(" ").filter(Boolean);
    if (!words.length) {
      return this.options;
    }
    return this.options.filter((o) => {
      const text = normalizeName(
        `${o.label} ${o.detail ?? ""} ${o.group ?? ""}`,
      );
      return words.every((w) => text.includes(w));
    });
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    this.close();
  }

  override updated() {
    if (this.open) {
      this.list
        ?.querySelector(".option.active")
        ?.scrollIntoView({ block: "nearest" });
    }
  }

  override render() {
    const filtered = this.filtered;
    const items: unknown[] = [];
    let group: string | undefined;
    filtered.forEach((o, i) => {
      if (o.group !== group) {
        group = o.group;
        if (group) {
          items.push(
            html`<li class="heading" role="presentation">${group}</li>`,
          );
        }
      }
      items.push(html`<li
        id="${this.listId}-${i}"
        class=${classMap({
          option: true,
          active: i === this.active,
          selected: o.value === this.value,
        })}
        role="option"
        aria-selected=${o.value === this.value ? "true" : "false"}
        @mousedown=${this.keepFocus}
        @click=${
          // eslint-disable-next-line lit/no-template-arrow
          () => this.pick(o)
        }
      >
        ${o.label}${
          o.detail ? html` <span class="detail">${o.detail}</span>` : nothing
        }
      </li>`);
    });
    const selected = this.selected;
    return html`<div class="field">
        <input
          type="text"
          role="combobox"
          autocomplete="off"
          aria-label=${this.label || this.placeholder}
          aria-expanded=${this.open ? "true" : "false"}
          aria-controls=${this.listId}
          aria-autocomplete="list"
          aria-activedescendant=${
            this.open && filtered.length ? `${this.listId}-${this.active}` : ""
          }
          placeholder=${selected ? selected.label : this.placeholder}
          .value=${this.open ? this.query : (selected?.label ?? "")}
          ?disabled=${this.disabled}
          @focus=${this.onFocus}
          @click=${this.onFocus}
          @input=${this.onInput}
          @keydown=${this.onKeydown}
          @blur=${this.close}
        />
        <span class="chevron" aria-hidden="true">expand_more</span>
      </div>
      <div popover="manual" @mousedown=${this.keepFocus}>
        <ul id=${this.listId} role="listbox" aria-label=${this.label}>
          ${items.length ? items : html`<li class="empty">Kein Treffer</li>`}
        </ul>
      </div>`;
  }

  /** Clicks in the list mustn't blur the input before the pick. */
  keepFocus(e: Event) {
    e.preventDefault();
  }

  async onFocus() {
    if (this.open || this.disabled) {
      return;
    }
    this.query = "";
    const index = this.filtered.findIndex((o) => o.value === this.value);
    this.active = Math.max(index, 0);
    this.open = true;
    await this.updateComplete;
    this.list?.showPopover?.();
    this.placed = "";
    this.follow();
  }

  onInput(e: InputEvent) {
    e.stopPropagation();
    this.query = (e.target as HTMLInputElement).value;
    this.active = 0;
    if (!this.open) {
      this.onFocus();
    }
  }

  onKeydown(e: KeyboardEvent) {
    const filtered = this.filtered;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        if (!this.open) {
          this.onFocus();
          return;
        }
        this.active = Math.min(this.active + 1, filtered.length - 1);
        break;
      case "ArrowUp":
        e.preventDefault();
        this.active = Math.max(this.active - 1, 0);
        break;
      case "Enter":
        if (this.open && filtered[this.active]) {
          e.preventDefault();
          this.pick(filtered[this.active]);
        }
        break;
      case "Escape":
        if (this.open) {
          e.preventDefault();
          this.close();
          this.input.blur();
        }
        break;
    }
  }

  pick(option: PickerOption) {
    this.close();
    this.input?.blur();
    if (option.value !== this.value) {
      this.dispatchEvent(
        new CustomEvent("change", { detail: { value: option.value } }),
      );
    }
  }

  close = () => {
    cancelAnimationFrame(this.frame);
    if (!this.open) {
      return;
    }
    this.open = false;
    this.query = "";
    try {
      this.list?.hidePopover?.();
    } catch {
      // Already hidden.
    }
  };

  /**
   * Keeps the list at the field while it is open. Scroll events don't leave
   * the shadow roots of the scrolling containers, so instead of listening,
   * the position is checked every frame.
   */
  follow = () => {
    if (!this.open) {
      return;
    }
    if (!this.place()) {
      // The field scrolled out of sight.
      this.close();
      this.input?.blur();
      return;
    }
    this.frame = requestAnimationFrame(this.follow);
  };

  /**
   * Puts the list below the field (or above it, if there's no room).
   * Returns `false` if the field is out of sight.
   */
  place() {
    const list = this.list;
    const input = this.input;
    if (!list || !input) {
      return false;
    }
    const box = input.getBoundingClientRect();
    if (!this.isVisible(box)) {
      return false;
    }
    const height = list.offsetHeight;
    const width = Math.max(box.width, list.offsetWidth);
    const below = innerHeight - box.bottom;
    const top =
      below < height + 8 && box.top > below
        ? Math.max(8, box.top - height - 4)
        : box.bottom + 4;
    const left = Math.max(8, Math.min(box.left, innerWidth - width - 8));
    const placed = `${top}/${left}/${box.width}`;
    if (placed !== this.placed) {
      this.placed = placed;
      list.style.top = `${top}px`;
      list.style.left = `${left}px`;
      list.style.minWidth = `${box.width}px`;
    }
    return true;
  }

  /**
   * Whether the field shows: inside the window and inside every scrolling
   * container around it, across shadow roots.
   */
  isVisible(box: DOMRect) {
    if (box.bottom < 0 || box.top > innerHeight) {
      return false;
    }
    let node: Node | null = this;
    while (node) {
      node = node instanceof ShadowRoot ? node.host : node.parentNode;
      if (node instanceof Element && node !== document.documentElement) {
        const overflow = getComputedStyle(node).overflowY;
        if (overflow === "auto" || overflow === "scroll") {
          const clip = node.getBoundingClientRect();
          if (box.bottom < clip.top || box.top > clip.bottom) {
            return false;
          }
        }
      }
    }
    return true;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-rider-picker": CupRiderPicker;
  }
}
