// A small History API router. It covers what the app needs (static and :param
// segments, links, back/forward) for about 1.1KB gzipped, against 9.2KB for
// @solidjs/router in the same test app (measured 2026-11-17, details in handoffs/josh.md).
import { createMemo, createSignal, type Component, type JSX } from "solid-js";

export type Params = Record<string, string>;
export type RouteComponent = Component<{ params: Params }>;
export type Route = { path: string; component: RouteComponent };

const [pathname, setPathname] = createSignal(
  typeof location === "undefined" ? "/" : location.pathname,
);

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => setPathname(location.pathname));
}

export { pathname };

/** Matches "/judge/:token" against "/judge/abc" and returns { token: "abc" }, or null. */
export function matchPath(pattern: string, path: string): Params | null {
  const want = pattern.split("/").filter(Boolean);
  const got = path.split("/").filter(Boolean);
  if (want.length !== got.length) return null;
  const params: Params = {};
  for (let i = 0; i < want.length; i++) {
    const w = want[i] as string;
    const g = got[i] as string;
    if (w.startsWith(":")) params[w.slice(1)] = decodeURIComponent(g);
    else if (w !== g) return null;
  }
  return params;
}

export function navigate(to: string, options: { replace?: boolean } = {}) {
  if (to === location.pathname) return;
  if (options.replace) history.replaceState(null, "", to);
  else history.pushState(null, "", to);
  setPathname(location.pathname);
  window.scrollTo(0, 0);
}

export function Router(props: { routes: Route[]; fallback: RouteComponent }) {
  const match = createMemo(() => {
    for (const route of props.routes) {
      const params = matchPath(route.path, pathname());
      if (params) return { component: route.component, params };
    }
    return { component: props.fallback, params: {} };
  });
  return (
    <>
      {(() => {
        const { component: Page, params } = match();
        return <Page params={params} />;
      })()}
    </>
  );
}

// Props are listed explicitly: spreading arbitrary attributes pulls in Solid's
// spread helper, which costs more than the whole router.
type LinkProps = {
  href: string;
  class?: string;
  "aria-label"?: string;
  "aria-current"?: "page" | undefined;
  children?: JSX.Element;
};

/** An <a> that navigates in-app for plain left clicks and leaves everything else to the browser. */
export function Link(props: LinkProps) {
  const onClick = (event: MouseEvent) => {
    if (
      event.defaultPrevented ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey ||
      !props.href.startsWith("/")
    ) {
      return;
    }
    event.preventDefault();
    navigate(props.href);
  };
  return (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      aria-current={props["aria-current"]}
      onClick={onClick}
    >
      {props.children}
    </a>
  );
}
