import { Navigate, useParams } from "react-router-dom";

/** `/entity/:id` is no longer its own page — its content (`EntityPanel`)
 *  is now Explore's own Entity tab, so the two share one picker instead
 *  of being two screens a reader has to hop between. This redirect exists
 *  only so a link built the old way (a saved bookmark, `contradictions.tsx`'s
 *  own "jump to entity" box, `trace.ts`'s route descriptors) still lands
 *  somewhere real — `openTargetFor` itself already points at
 *  `/explore/:id?view=entity` directly and never constructs this path. */
export default function EntityRedirect() {
  const { id } = useParams<{ id?: string }>();
  return <Navigate to={id ? `/explore/${encodeURIComponent(id)}?view=entity` : "/explore"} replace />;
}
