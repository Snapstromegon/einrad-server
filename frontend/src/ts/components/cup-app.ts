import { Task } from "@lit/task";
import { html, LitElement, nothing, type TemplateResult } from "lit";
import { customElement, state } from "lit/decorators.js";
import "./views/cup-view-not-found.js";
import "./elements/cup-context-user.js";
import "./elements/cup-context-system-status.js";
if (!globalThis.URLPattern) {
  await import("urlpattern-polyfill");
}

interface Route {
  path: URLPattern;
  load: () => Promise<unknown>;
  render: (urlPatternExec: URLPatternResult | null) => TemplateResult;
}

const authed = (x: TemplateResult) =>
  html`<cup-context-user>${x}</cup-context-user>`;

/**
 * An organizer page: logged in, inside the admin shell. The shell only
 * renders `view` for users whose roles open the page, so it doesn't load or
 * poll anything otherwise. `bare` leaves out the shell's bar.
 */
const admin = (
  view: () => TemplateResult,
  { fullscreen = false, bare = false } = {},
) =>
  authed(
    html`<cup-admin-shell
      ?fullscreen=${fullscreen}
      ?bare=${bare}
      .view=${view}
    ></cup-admin-shell>`,
  );

/** Loads an admin view together with the admin shell. */
const loadAdmin = (view: () => Promise<unknown>) => () =>
  Promise.all([import("./elements/cup-admin-shell.js"), view()]);

@customElement("cup-app")
export default class CupApp extends LitElement {
  @state() location: string = location.pathname;

  private routes: Route[] = [
    {
      path: new URLPattern({ pathname: "/" }),
      load: () => import("./views/cup-view-home.js"),
      render: () => authed(html`<cup-view-home></cup-view-home>`),
    },
    {
      path: new URLPattern({ pathname: "/acts" }),
      load: () => import("./views/cup-view-acts.js"),
      render: () => authed(html`<cup-view-acts></cup-view-acts>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-music-control" }),
      load: loadAdmin(() => import("./views/cup-view-admin-music-control.js")),
      render: () =>
        admin(
          () =>
            html`<cup-view-admin-music-control></cup-view-admin-music-control>`,
          { fullscreen: true },
        ),
    },
    {
      path: new URLPattern({ pathname: "/admin-awards" }),
      load: loadAdmin(() => import("./views/cup-view-admin-awards.js")),
      render: () =>
        admin(() => html`<cup-view-admin-awards></cup-view-admin-awards>`, {
          fullscreen: true,
        }),
    },
    {
      // The web player of remote playback, e.g. on the sound-desk laptop.
      path: new URLPattern({ pathname: "/player" }),
      load: loadAdmin(() => import("./views/cup-view-player.js")),
      render: () => admin(() => html`<cup-view-player></cup-view-player>`),
    },
    {
      path: new URLPattern({ pathname: "/admin" }),
      load: loadAdmin(() => import("./views/cup-view-admin.js")),
      render: () => admin(() => html`<cup-view-admin></cup-view-admin>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-competition" }),
      load: loadAdmin(() => import("./views/cup-view-admin-competition.js")),
      render: () =>
        admin(
          () => html`<cup-view-admin-competition></cup-view-admin-competition>`,
        ),
    },
    {
      path: new URLPattern({ pathname: "/admin-check-in" }),
      load: loadAdmin(() => import("./views/cup-view-admin-check-in.js")),
      render: () =>
        admin(() => html`<cup-view-admin-check-in></cup-view-admin-check-in>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-riders" }),
      load: loadAdmin(() => import("./views/cup-view-admin-riders.js")),
      render: () =>
        admin(() => html`<cup-view-admin-riders></cup-view-admin-riders>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-payments" }),
      load: loadAdmin(() => import("./views/cup-view-admin-payments.js")),
      render: () =>
        admin(() => html`<cup-view-admin-payments></cup-view-admin-payments>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-acts-overview" }),
      load: loadAdmin(() => import("./views/cup-view-admin-acts-overview.js")),
      render: () =>
        admin(
          () =>
            html`<cup-view-admin-acts-overview></cup-view-admin-acts-overview>`,
        ),
    },
    {
      path: new URLPattern({ pathname: "/admin-categories" }),
      load: loadAdmin(() => import("./views/cup-view-admin-categories.js")),
      render: () =>
        admin(
          () => html`<cup-view-admin-categories></cup-view-admin-categories>`,
        ),
    },
    {
      path: new URLPattern({ pathname: "/admin-timeplan" }),
      load: loadAdmin(() => import("./views/cup-view-admin-timeplan.js")),
      render: () =>
        admin(() => html`<cup-view-admin-timeplan></cup-view-admin-timeplan>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-medals" }),
      load: loadAdmin(() => import("./views/cup-view-admin-medals.js")),
      render: () =>
        admin(() => html`<cup-view-admin-medals></cup-view-admin-medals>`),
    },
    {
      path: new URLPattern({ pathname: "/admin-judges" }),
      load: loadAdmin(() => import("./views/cup-view-admin-judges.js")),
      render: () =>
        admin(() => html`<cup-view-admin-judges></cup-view-admin-judges>`),
    },
    {
      path: new URLPattern({ pathname: "/register" }),
      load: () => import("./views/cup-view-register.js"),
      render: () => html`<cup-view-register></cup-view-register>`,
    },
    {
      path: new URLPattern({ pathname: "/verify_email" }),
      load: () => import("./views/cup-view-verify-email.js"),
      render: () => html`<cup-view-verify-email></cup-view-verify-email>`,
    },
    {
      path: new URLPattern({ pathname: "/request_password_reset" }),
      load: () => import("./views/cup-view-request-password-reset.js"),
      render: () =>
        html`<cup-view-request-password-reset></cup-view-request-password-reset>`,
    },
    {
      path: new URLPattern({ pathname: "/reset_password" }),
      load: () => import("./views/cup-view-reset-password.js"),
      render: () => html`<cup-view-reset-password></cup-view-reset-password>`,
    },
    {
      path: new URLPattern({ pathname: "/create_club" }),
      load: () => import("./views/cup-view-create-club.js"),
      render: () => html`<cup-view-create-club></cup-view-create-club>`,
    },
    {
      path: new URLPattern({ pathname: "/startlist" }),
      load: () => import("./views/cup-view-startlist.js"),
      render: () => html`<cup-view-startlist></cup-view-startlist>`,
    },
    {
      path: new URLPattern({ pathname: "/info-board" }),
      load: () => import("./views/cup-view-info-board.js"),
      render: () => html`<cup-view-info-board></cup-view-info-board>`,
    },
    {
      path: new URLPattern({ pathname: "/admin-moderation" }),
      load: loadAdmin(() => import("./views/cup-view-admin-moderation.js")),
      render: () =>
        admin(
          () => html`<cup-view-admin-moderation></cup-view-admin-moderation>`,
          { bare: true },
        ),
    },
  ];

  get currentRoute() {
    return this.routes.find((route) =>
      route.path.test(this.location, location.origin),
    );
  }

  private routeTask = new Task(this, {
    task: async () => {
      console.log("Loading route...", this.location, location.pathname);
      if (!this.currentRoute) {
        throw new Error("Current Route does not exist.");
      }
      await this.currentRoute.load();
      return {
        route: this.currentRoute,
        urlPatternExec: this.currentRoute.path.exec(
          this.location,
          location.origin,
        ),
      };
    },
    args: () => [this.location],
  });

  override render() {
    return this.routeTask.render({
      pending: () => html`<div>Lädt…</div>`,
      error: () =>
        html`<cup-view-not-found .url=${this.location}></cup-view-not-found>`,
      complete: ({ route, urlPatternExec }) =>
        html`<cup-context-system-status
          >${
            route ? route.render(urlPatternExec) : nothing
          }</cup-context-system-status
        >`,
    });
  }
}
