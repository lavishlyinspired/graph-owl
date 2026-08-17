import { lazy } from "react";
import { createBrowserRouter, Navigate } from "react-router-dom";
import { AppShell } from "./chrome/AppShell";
import { ROUTES } from "./lib/routes";

/** Plan 122a A0 / `00f-ui-architecture.md` §"Bundle measurement": every
 *  route is its own dynamic import so G6, React Flow and the SPARQL editor
 *  (added in later slices) load only on the routes that need them. This is
 *  the lever `00f` names for the 747KB-over-budget measurement on `ui/` —
 *  applied from the first commit here, not retrofitted. */
const routeModules = import.meta.glob("./routes/*.tsx");

function lazyRoute(name: string) {
  const loader = routeModules[`./routes/${name}.tsx`];
  if (!loader) throw new Error(`no route module for "${name}"`);
  return lazy(loader as () => Promise<{ default: React.ComponentType }>);
}

// Vite sets `import.meta.env.BASE_URL` from the `base` config option
// (`/` in dev, `/next/` in the temporary A0 embed build) — reusing it here
// means the router's basename can never drift from where the assets
// actually are, without a second env var to keep in sync.
export const router = createBrowserRouter(
  [
    {
      path: "/",
      element: <AppShell />,
      children: [
        { index: true, element: <Navigate to={`/${ROUTES[0]}`} replace /> },
        ...ROUTES.map((route) => {
          const Component = lazyRoute(route);
          return { path: route, element: <Component /> };
        }),
      ],
    },
  ],
  { basename: import.meta.env.BASE_URL },
);
