interface PlaceholderPageProps {
  readonly title: string;
  readonly slice: string;
}

/** Deliberately honest, not a finished screen: `plans/122-frontend-rebuild.md`
 *  constraint "no screen merges rendering a panel it cannot populate" is
 *  about screens claiming to be done while silently empty. This says
 *  outright which slice builds it, so it cannot be mistaken for one. */
export function PlaceholderPage({ title, slice }: PlaceholderPageProps) {
  return (
    <div className="p-8">
      <h1 className="text-[22px] font-semibold text-gowl-t1">{title}</h1>
      <p className="mt-2 text-[17px] text-gowl-t5">{`Ships in plans/122a-graphowl-app.md, slice ${slice}.`}</p>
    </div>
  );
}
