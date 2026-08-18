import { lazy } from "react";
import { createBrowserRouter, Navigate } from "react-router-dom";
import { AppShell } from "./chrome/AppShell";
import { ROUTES } from "./lib/routes";

const routeModules = import.meta.glob("./routes/*.tsx");

function lazyRoute(name: string) {
  const loader = routeModules[`./routes/${name}.tsx`];
  if (!loader) throw new Error(`no route module for "${name}"`);
  return lazy(loader as () => Promise<{ default: React.ComponentType }>);
}

export const router = createBrowserRouter([
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
]);
