/** A subject id rendered as a link that opens its neighbourhood — Plan 113
 *  Slice C, linking `SubjectExplorer` in from the places this console
 *  already renders a bare subject and does nothing with it on click.
 *
 *  **Self-contained on purpose.** The findings queue is a generic
 *  `QueueConfig` shared by every domain pack (`plans/105-domain-neutrality.md`);
 *  lifting "which subject is being explored" into that queue's own state
 *  would tie a GST-shaped concern to a config every pack renders through.
 *  A `Drawer` owned by this one component avoids that coupling entirely. */

import { useState } from "react";
import { Button, Drawer } from "./../components/ui/antd-compat";
import { SubjectExplorer } from "./SubjectExplorer";

export function ClickableSubject({ seed, label }: { seed: string; label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button type="link" size="small" style={{ padding: 0, height: "auto" }} onClick={() => setOpen(true)}>
        {label}
      </Button>
      <Drawer title={label} open={open} onClose={() => setOpen(false)} width={520} destroyOnHidden>
        <SubjectExplorer seed={seed} label={label} />
      </Drawer>
    </>
  );
}
