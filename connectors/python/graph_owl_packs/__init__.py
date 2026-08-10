"""Domain packs — the mechanism by which a domain is data rather than code.

A pack is a directory: a `pack.toml` naming its vocabulary, RDF documents to
land, and configuration for matching, findings and the console. The loader
here reads that and drives graph-owl's public HTTP surface. **Nothing in this
package knows what a hospital, a hotel or a tax return is**, which is the
claim `plans/105-domain-neutrality.md` exists to make true and
`packs/hospitality/` exists to test.
"""

from .erpnext import ErpnextClient, ErpnextError, SyncResult, sync_doctype
from .loader import DocumentResult, LoadError, LoadResult, load_pack
from .manifest import Document, Manifest, ManifestError, Predicate

__all__ = [
    "Document",
    "ErpnextClient",
    "ErpnextError",
    "DocumentResult",
    "LoadError",
    "LoadResult",
    "Manifest",
    "ManifestError",
    "Predicate",
    "SyncResult",
    "sync_doctype",
    "load_pack",
]
